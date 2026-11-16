//! What the servers keep for a while: the rate limits' counts, and reads
//! worth not repeating, such as the leaderboard and players' ratings.
//!
//! In DragonflyDB, or any server that speaks Redis's protocol, when the
//! config's `[cache]` names one, so every server sharing it counts the
//! same. Without one, and whenever it doesn't answer in time, the limits
//! count in this server's own memory, and still hold, for this server;
//! reads go to the database afresh. Nobody waits on a cache that's down,
//! and a warning says so, now and then.
//!
//! Times are Unix times in seconds, passed in, so tests can drive the
//! clock; what's kept runs out by the real clock.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, Weak};
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use redis::aio::{ConnectionManager, ConnectionManagerConfig};
use redis::{Cmd, FromRedisValue, Pipeline};
use tracing::{info, warn};

use crate::config::CacheConfig;

/// How long the cache gets to answer before this server answers from its
/// memory instead.
const ANSWER_WAIT: Duration = Duration::from_millis(250);
/// After the cache fails, how long this server keeps to its memory before
/// asking it again.
const RETRY_AFTER: Duration = Duration::from_secs(5);
/// How often a cache that's down is warned about.
const WARN_EVERY: Duration = Duration::from_secs(60);
/// The memory is swept of what ran out every so many writes.
const SWEEP_EVERY: u64 = 1024;

/// Requests answered from memory because the cache didn't answer.
pub static FALLBACKS: AtomicU64 = AtomicU64::new(0);

#[derive(Clone)]
pub struct Cache(Arc<Inner>);

struct Inner {
    /// Before every key, so servers that share a cache but not players
    /// keep apart.
    prefix: String,
    memory: Memory,
    remote: Option<Remote>,
}

#[derive(Default)]
struct Remote {
    conn: Mutex<Option<ConnectionManager>>,
    /// After failing, it's left alone until then.
    resting_until: Mutex<Option<Instant>>,
    warned_at: Mutex<Option<Instant>>,
    /// It answered the last time it was asked.
    up: AtomicBool,
}

impl Remote {
    /// The connection to ask, unless it's down or resting.
    fn ready(&self) -> Option<ConnectionManager> {
        let resting = self.resting_until.lock().unwrap();
        if resting.is_some_and(|until| Instant::now() < until) {
            return None;
        }
        drop(resting);
        self.conn.lock().unwrap().clone()
    }

    fn answered(&self) {
        if !self.up.swap(true, Ordering::Relaxed) {
            info!("the cache answers; counting there");
        }
    }

    fn failed(&self, why: &str) {
        self.up.store(false, Ordering::Relaxed);
        *self.resting_until.lock().unwrap() = Some(Instant::now() + RETRY_AFTER);
        let mut warned = self.warned_at.lock().unwrap();
        if warned.is_none_or(|at| at.elapsed() >= WARN_EVERY) {
            *warned = Some(Instant::now());
            warn!(
                error = why,
                "the cache isn't answering; using this server's memory"
            );
        }
    }
}

impl Cache {
    /// This server's memory alone.
    pub fn memory() -> Cache {
        Cache(Arc::new(Inner {
            prefix: String::new(),
            memory: Memory::default(),
            remote: None,
        }))
    }

    /// The cache `cfg` names, connected in the background: until it
    /// answers, and whenever it doesn't, this server's memory stands in.
    /// Must run inside an actix `System`.
    pub fn connect(cfg: &CacheConfig) -> Result<Cache> {
        let client = redis::Client::open(cfg.url.as_str())
            .context("the cache's url, in [cache], isn't one this server can use")?;
        let cache = Cache(Arc::new(Inner {
            prefix: format!("{}:", cfg.prefix),
            memory: Memory::default(),
            remote: Some(Remote::default()),
        }));
        actix_rt::spawn(keep_connecting(client, Arc::downgrade(&cache.0)));
        Ok(cache)
    }

    /// Whether the cache answers now; `None` without one.
    pub fn up(&self) -> Option<bool> {
        let remote = self.0.remote.as_ref()?;
        Some(remote.up.load(Ordering::Relaxed))
    }

    fn key(&self, key: &str) -> String {
        format!("{}{key}", self.0.prefix)
    }

    /// Asks the cache, if it's there and up; `None` to answer from memory.
    async fn ask<T: FromRedisValue>(&self, request: Request<'_>) -> Option<T> {
        let remote = self.0.remote.as_ref()?;
        let mut conn = remote.ready();
        let Some(conn) = conn.as_mut() else {
            FALLBACKS.fetch_add(1, Ordering::Relaxed);
            return None;
        };
        let answer = match request {
            Request::One(cmd) => tokio::time::timeout(ANSWER_WAIT, cmd.query_async(conn)).await,
            Request::All(pipe) => tokio::time::timeout(ANSWER_WAIT, pipe.query_async(conn)).await,
        };
        match answer {
            Ok(Ok(value)) => {
                remote.answered();
                Some(value)
            }
            Ok(Err(e)) => {
                remote.failed(&e.to_string());
                FALLBACKS.fetch_add(1, Ordering::Relaxed);
                None
            }
            Err(_) => {
                remote.failed("no answer in time");
                FALLBACKS.fetch_add(1, Ordering::Relaxed);
                None
            }
        }
    }

    // Counts, for the rate limits.

    /// Notes something counted under `key` happening at `at`, counted for
    /// `keep`.
    pub async fn note(&self, key: &str, at: i64, keep: Duration) {
        self.note_and_count(key, at, keep, at).await;
    }

    /// Notes something at `at`, counted for `keep`, then answers how many
    /// under `key` happened after `since`, those two at once: so a flood
    /// sent together can't all slip under a limit.
    pub async fn note_and_count(&self, key: &str, at: i64, keep: Duration, since: i64) -> i64 {
        let key = self.key(key);
        let keep_secs = secs(keep);
        let mut pipe = redis::pipe();
        pipe.atomic()
            .zrembyscore(&key, "-inf", format!("({}", at - keep_secs))
            .ignore()
            .zadd(&key, member(at), at)
            .ignore()
            .expire(&key, keep_secs.max(1))
            .ignore()
            .zcount(&key, format!("({since}"), "+inf");
        match self.ask::<(i64,)>(Request::All(&pipe)).await {
            Some((count,)) => count,
            None => self.0.memory.note_and_count(&key, at, keep, since),
        }
    }

    /// How many things under `key` happened after `since`.
    pub async fn count(&self, key: &str, since: i64) -> i64 {
        let key = self.key(key);
        let cmd = redis::cmd("ZCOUNT")
            .arg(&key)
            .arg(format!("({since}"))
            .arg("+inf")
            .to_owned();
        match self.ask::<i64>(Request::One(&cmd)).await {
            Some(count) => count,
            None => self.0.memory.count(&key, since),
        }
    }

    /// Forgets `key`, whatever it holds.
    pub async fn forget(&self, key: &str) {
        let key = self.key(key);
        let cmd = redis::cmd("DEL").arg(&key).to_owned();
        if self.ask::<i64>(Request::One(&cmd)).await.is_none() {
            self.0.memory.forget(&key);
        }
    }

    /// Holds `key` until `until`, it being `now`: a lock that runs out.
    pub async fn hold(&self, key: &str, until: i64, now: i64) {
        let key = self.key(key);
        let left = (until - now).max(1);
        let cmd = redis::cmd("SET")
            .arg(&key)
            .arg(until)
            .arg("EX")
            .arg(left)
            .to_owned();
        if self.ask::<()>(Request::One(&cmd)).await.is_none() {
            let keep = Duration::from_secs(left as u64);
            self.0.memory.put(&key, Value::Number(until), keep);
        }
    }

    /// Until when `key` is held, if it is.
    pub async fn held_until(&self, key: &str) -> Option<i64> {
        let key = self.key(key);
        let cmd = redis::cmd("GET").arg(&key).to_owned();
        match self.ask::<Option<i64>>(Request::One(&cmd)).await {
            Some(until) => until,
            None => self.0.memory.number(&key),
        }
    }

    // Reads kept for a while: only in a shared cache. One server's memory
    // can't keep them, as it never hears when another server changes what
    // they read.

    /// What the shared cache keeps under `key`; `None` if it keeps nothing
    /// there, or doesn't answer.
    pub async fn get(&self, key: &str) -> Option<Vec<u8>> {
        let key = self.key(key);
        let cmd = redis::cmd("GET").arg(&key).to_owned();
        self.ask::<Option<Vec<u8>>>(Request::One(&cmd)).await?
    }

    /// Has the shared cache keep `value` under `key` for `keep`, if there
    /// is one.
    pub async fn put(&self, key: &str, value: &[u8], keep: Duration) {
        let key = self.key(key);
        let ms = keep.as_millis().max(1) as u64;
        let cmd = redis::cmd("SET")
            .arg(&key)
            .arg(value)
            .arg("PX")
            .arg(ms)
            .to_owned();
        self.ask::<()>(Request::One(&cmd)).await;
    }

    /// The count under `key`, 0 until [`Self::bump`]ed: a generation, for
    /// keys of things that change all at once. `None` without a shared
    /// cache that answers: read afresh then.
    pub async fn generation(&self, key: &str) -> Option<i64> {
        let key = self.key(key);
        let cmd = redis::cmd("GET").arg(&key).to_owned();
        let generation = self.ask::<Option<i64>>(Request::One(&cmd)).await?;
        Some(generation.unwrap_or(0))
    }

    /// Moves the generation under `key` on, so what was kept for the old one
    /// is never read again.
    pub async fn bump(&self, key: &str) {
        let key = self.key(key);
        let cmd = redis::cmd("INCR").arg(&key).to_owned();
        self.ask::<i64>(Request::One(&cmd)).await;
    }
}

/// One command, or several at once.
enum Request<'a> {
    One(&'a Cmd),
    All(&'a Pipeline),
}

/// Connects to the cache, and tries again every few seconds until it can,
/// for as long as the cache is in use.
async fn keep_connecting(client: redis::Client, cache: Weak<Inner>) {
    let config = ConnectionManagerConfig::new()
        .set_connection_timeout(Some(ANSWER_WAIT * 4))
        .set_response_timeout(Some(ANSWER_WAIT))
        .set_number_of_retries(1)
        .set_max_delay(RETRY_AFTER);
    loop {
        let connected = ConnectionManager::new_with_config(client.clone(), config.clone()).await;
        let Some(cache) = cache.upgrade() else {
            return;
        };
        let remote = cache.remote.as_ref().expect("a cache to connect to");
        match connected {
            Ok(conn) => {
                *remote.conn.lock().unwrap() = Some(conn);
                *remote.resting_until.lock().unwrap() = None;
                remote.answered();
                return;
            }
            Err(e) => remote.failed(&e.to_string()),
        }
        drop(cache);
        tokio::time::sleep(RETRY_AFTER).await;
    }
}

fn secs(d: Duration) -> i64 {
    d.as_secs() as i64
}

/// A new member for a count's set: the time, and something no other server
/// or moment picks, as two things can happen in the same second.
fn member(at: i64) -> String {
    format!("{at}:{:x}", rand::random::<u64>())
}

/// What the memory keeps under a key.
enum Value {
    /// When each counted thing happened.
    Times(Vec<i64>),
    /// When a hold runs out.
    Number(i64),
}

struct Entry {
    value: Value,
    until: Instant,
}

/// This server's memory, standing in for the cache.
#[derive(Default)]
struct Memory {
    entries: Mutex<HashMap<String, Entry>>,
    writes: AtomicU64,
}

impl Memory {
    fn note_and_count(&self, key: &str, at: i64, keep: Duration, since: i64) -> i64 {
        let mut entries = self.write();
        let now = Instant::now();
        let entry = entries
            .entry(key.to_string())
            .and_modify(|e| {
                if e.until <= now {
                    e.value = Value::Times(Vec::new());
                }
            })
            .or_insert(Entry {
                value: Value::Times(Vec::new()),
                until: now,
            });
        if !matches!(entry.value, Value::Times(_)) {
            entry.value = Value::Times(Vec::new());
        }
        let Value::Times(times) = &mut entry.value else {
            unreachable!()
        };
        let oldest = at - secs(keep);
        times.retain(|&t| t >= oldest);
        times.push(at);
        entry.until = now + keep.max(Duration::from_secs(1));
        times.iter().filter(|&&t| t > since).count() as i64
    }

    fn count(&self, key: &str, since: i64) -> i64 {
        self.read(key, |v| match v {
            Value::Times(times) => Some(times.iter().filter(|&&t| t > since).count() as i64),
            _ => None,
        })
        .unwrap_or_default()
    }

    fn forget(&self, key: &str) {
        self.write().remove(key);
    }

    fn put(&self, key: &str, value: Value, keep: Duration) {
        let until = Instant::now() + keep;
        self.write().insert(key.to_string(), Entry { value, until });
    }

    fn number(&self, key: &str) -> Option<i64> {
        self.read(key, |v| match v {
            Value::Number(n) => Some(*n),
            _ => None,
        })
    }

    /// What `see` makes of the value under `key`, if it hasn't run out.
    fn read<T>(&self, key: &str, see: impl FnOnce(&Value) -> Option<T>) -> Option<T> {
        let entries = self.entries.lock().unwrap();
        let entry = entries.get(key).filter(|e| e.until > Instant::now())?;
        see(&entry.value)
    }

    /// The entries, to change: swept of what ran out now and then.
    fn write(&self) -> std::sync::MutexGuard<'_, HashMap<String, Entry>> {
        let mut entries = self.entries.lock().unwrap();
        if self.writes.fetch_add(1, Ordering::Relaxed) % SWEEP_EVERY == SWEEP_EVERY - 1 {
            let now = Instant::now();
            entries.retain(|_, e| e.until > now);
        }
        entries
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// What every cache must do for the limits, in memory or in DragonflyDB.
    async fn counts_and_holds(cache: &Cache, prefix: &str) {
        let key = |k: &str| format!("{prefix}{k}");
        let minute = Duration::from_secs(60);
        // Counts in a window that slides.
        assert_eq!(cache.count(&key("c"), 0).await, 0);
        for at in [100, 100, 130] {
            cache.note(&key("c"), at, minute).await;
        }
        assert_eq!(cache.count(&key("c"), 99).await, 3);
        assert_eq!(cache.count(&key("c"), 100).await, 1, "after 100");
        assert_eq!(
            cache.note_and_count(&key("c"), 170, minute, 110).await,
            2,
            "130 and 170; the 100s slid out"
        );
        cache.forget(&key("c")).await;
        assert_eq!(cache.count(&key("c"), 0).await, 0);
        // Holds that run out.
        assert_eq!(cache.held_until(&key("h")).await, None);
        cache.hold(&key("h"), 1_000_900, 1_000_000).await;
        assert_eq!(cache.held_until(&key("h")).await, Some(1_000_900));
    }

    /// What a shared cache keeps for reads: and one that isn't there keeps
    /// nothing.
    async fn keeps_reads(cache: &Cache, prefix: &str, shared: bool) {
        let key = |k: &str| format!("{prefix}{k}");
        assert_eq!(cache.get(&key("v")).await, None);
        cache
            .put(&key("v"), b"page", Duration::from_millis(300))
            .await;
        let page = shared.then(|| b"page".to_vec());
        assert_eq!(cache.get(&key("v")).await, page);
        tokio::time::sleep(Duration::from_millis(500)).await;
        assert_eq!(cache.get(&key("v")).await, None, "run out");
        let generation = |n| shared.then_some(n);
        assert_eq!(cache.generation(&key("g")).await, generation(0));
        cache.bump(&key("g")).await;
        cache.bump(&key("g")).await;
        assert_eq!(cache.generation(&key("g")).await, generation(2));
    }

    #[actix_rt::test]
    async fn the_memory_counts_and_holds_but_keeps_no_reads() {
        let cache = Cache::memory();
        counts_and_holds(&cache, "").await;
        keeps_reads(&cache, "", false).await;
    }

    #[actix_rt::test]
    async fn dragonfly_counts_holds_and_keeps_reads() {
        let url = snaketui_test_services::dragonfly();
        let cache = Cache::connect(&CacheConfig {
            url,
            prefix: "test".into(),
        })
        .unwrap();
        let deadline = Instant::now() + Duration::from_secs(10);
        while cache.up() != Some(true) {
            assert!(Instant::now() < deadline, "DragonflyDB never answered");
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        // Each run of the tests keeps to keys of its own.
        let run = format!("{:x}:", rand::random::<u64>());
        counts_and_holds(&cache, &run).await;
        keeps_reads(&cache, &run, true).await;
        assert_eq!(cache.up(), Some(true), "it answered everything itself");
    }

    #[actix_rt::test]
    async fn a_cache_that_never_answers_leaves_it_to_memory() {
        // Nothing listens on port 1.
        let cache = Cache::connect(&CacheConfig {
            url: "redis://127.0.0.1:1".into(),
            prefix: "test".into(),
        })
        .unwrap();
        let started = Instant::now();
        counts_and_holds(&cache, "").await;
        keeps_reads(&cache, "", false).await;
        assert!(
            started.elapsed() < Duration::from_secs(2),
            "nobody waited on it: {:?}",
            started.elapsed()
        );
        assert_eq!(cache.up(), Some(false));
    }

    #[test]
    fn a_url_that_isnt_one_is_refused() {
        let cfg = CacheConfig {
            url: "http://example.com".into(),
            prefix: "test".into(),
        };
        let system = actix::System::new();
        system.block_on(async {
            assert!(Cache::connect(&cfg).is_err());
        });
    }
}

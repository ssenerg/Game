//! Snake as the app shows it: single player on its own screens, and its
//! part of rooms. The local game drives the same [`SnakeUi`] on its own.

use std::time::Duration;

use game_api::codec::encode;
use game_api::{AnyView, PlayerId};
use ratatui::Frame;
use ratatui::layout::Size;
use ratatui::style::Color;
use ratatui::text::Span;
use shell_tui::online::{Category, InRoom};
use shell_tui::{Ctx, GameFx, GameUi, Key, Next, QuickStart, Reply, Saved};
use snake_core::config::{GameConfig, Rules, TICK_MS_MAX, TICK_MS_MIN, TICK_MS_STEP};
use snake_core::geom::Direction;
use snake_core::records::Records;
use snake_core::round::{
    GUNS_POOL, GUNS_RATING, PhaseView, QUICK_POOL, QUICK_RATING, RacerView, RoomView, SeatView,
    Snake, SnakeInput, max_players_for,
};
use snake_core::settings::{SETTINGS, room_settings};
use snake_core::solo::SoloGame;

use crate::ui;

/// Quick play's pools, as the player picks one: the pool, its name, and
/// what it is.
pub const QUICK_POOLS: [(&str, &str, &str); 2] = [
    (QUICK_POOL, "Classic", "every snake for itself"),
    (GUNS_POOL, "Guns", "lasers and pistols on the board"),
];

/// The prefs line that keeps the quick-play pool last picked, after the
/// settings' own. It's left out for the first, so prefs saved before guns
/// read the same.
const QUICK_PREF: &str = "quick=";

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SnakeScreen {
    /// Single-player settings. The local game starts here.
    Menu,
    Playing,
    Paused {
        selected: usize,
    },
    GameOver {
        new_record: bool,
    },
    /// A private room's rules, at [`room_settings`] entry `selected`. The
    /// host changes them.
    RoomRules {
        selected: usize,
    },
    /// Quick play's pools, at [`QUICK_POOLS`] entry `selected`.
    QuickPlay {
        selected: usize,
    },
}

pub struct SnakeUi {
    pub cfg: GameConfig,
    /// The setting picked on the settings screen, by its place in
    /// [`SETTINGS`].
    pub selected: usize,
    pub game: Option<SoloGame>,
    pub records: Records,
    /// The best on the rules being played, as the game started.
    pub record: u32,
    pub screen: SnakeScreen,
    /// Set by the first press of the reset key; a second press resets.
    pub confirm_reset: bool,
    /// The terminal the game is drawn on. Set it with [`GameUi::resize`].
    pub size: Size,
    /// The [`QUICK_POOLS`] entry quick play opens at: the one last played.
    pub quick: usize,
}

impl Default for SnakeUi {
    fn default() -> Self {
        Self::new(GameConfig::default(), Records::default())
    }
}

/// A room's view, if it's Snake's.
pub(crate) fn snake_view(room: &InRoom) -> Option<&RoomView> {
    room.view.of::<Snake>()
}

/// The snake `seat` has this round.
pub(crate) fn racer<'a>(view: &'a RoomView, seat: &SeatView) -> Option<&'a RacerView> {
    seat.slot
        .and_then(|slot| view.game.racers.get(slot as usize))
}

impl SnakeUi {
    /// Snake with these settings and records, at the settings screen.
    pub fn new(cfg: GameConfig, records: Records) -> Self {
        Self {
            cfg,
            selected: 0,
            game: None,
            records,
            record: 0,
            screen: SnakeScreen::Menu,
            confirm_reset: false,
            size: Size::new(0, 0),
            quick: 0,
        }
    }

    /// The prefs as saved: the settings, and the quick-play pool if it
    /// isn't the first.
    fn prefs(&self) -> String {
        let mut text = self.cfg.to_saved_text();
        if self.quick > 0 {
            text.push_str(&format!("{QUICK_PREF}{}\n", QUICK_POOLS[self.quick].0));
        }
        text
    }

    /// How long the local game may wait for a key before ticking.
    pub fn poll_interval(&self) -> Duration {
        if self.screen == SnakeScreen::Playing {
            Duration::from_millis(15)
        } else {
            Duration::from_millis(100)
        }
    }

    /// The snake must never move while its board isn't on screen.
    fn pause_if_hidden(&mut self) {
        if self.screen == SnakeScreen::Playing
            && self
                .game
                .as_ref()
                .is_some_and(|g| !ui::board_fits(g.world.width, g.world.height, self.size))
        {
            self.screen = SnakeScreen::Paused { selected: 0 };
        }
    }

    /// Starts a game by the settings, or says why it can't.
    fn start_game(&mut self, reply: &mut Reply) {
        let Ok(game) = SoloGame::new(self.cfg.rules.clone()) else {
            let bots = self.cfg.rules.clone().for_solo().bots;
            let s = if bots == 1 { "" } else { "s" };
            reply.notice = Some(format!(
                "The snake and {bots} bot{s} don't fit on this board. Make it bigger, \
                 or play fewer bots."
            ));
            return;
        };
        reply.fx.push(GameFx::SavePrefs(self.prefs()));
        self.record = self.records.get(&game.rules().record_key());
        self.game = Some(game);
        self.screen = SnakeScreen::Playing;
        self.pause_if_hidden();
    }

    fn save_record(&mut self, fx: &mut Vec<GameFx>) -> bool {
        let Some(game) = &self.game else { return false };
        let len = game.len() as u32;
        let key = game.rules().record_key();
        let new_record = self.records.update(&key, len);
        if new_record {
            fx.push(GameFx::Record { key, best: len });
        }
        self.record = self.record.max(len);
        new_record
    }

    fn quit_to_menu(&mut self, fx: &mut Vec<GameFx>) {
        self.save_record(fx);
        self.game = None;
        self.screen = SnakeScreen::Menu;
    }

    fn menu_key(&mut self, key: Key, reply: &mut Reply) {
        let confirm_reset = std::mem::take(&mut self.confirm_reset);
        match key {
            Key::Esc | Key::Char('q') => reply.next = Next::Home,
            Key::Up | Key::Down | Key::Char('w' | 'k' | 's' | 'j') => {
                self.selected = key.step(self.selected, SETTINGS.len()).unwrap_or(0);
            }
            Key::Left | Key::Char('a' | 'h') | Key::Right | Key::Char('d' | 'l') => {
                let back = matches!(key, Key::Left | Key::Char('a' | 'h'));
                self.cfg.adjust(self.selected, if back { -1 } else { 1 });
                self.cfg.rules.bot_for_guns();
            }
            Key::Char('r') if confirm_reset => self.cfg = GameConfig::default(),
            Key::Char('r') => self.confirm_reset = true,
            Key::Enter => self.start_game(reply),
            _ => {}
        }
    }

    /// The room's rules, in the lobby: the host changes them with ←→.
    fn room_rules_key(&mut self, selected: usize, key: Key, room: Option<&InRoom>) -> Reply {
        let Some((room, view)) = room.and_then(|r| Some((r, snake_view(r)?))) else {
            return Reply::to(Next::Room);
        };
        let items = room_settings();
        if let Some(selected) = key.step(selected, items.len()) {
            self.screen = SnakeScreen::RoomRules { selected };
            return Reply::default();
        }
        match key {
            Key::Left | Key::Right | Key::Char('a' | 'h' | 'd' | 'l') if room.hosting() => {
                let delta = if matches!(key, Key::Left | Key::Char('a' | 'h')) {
                    -1
                } else {
                    1
                };
                let mut cfg = GameConfig {
                    rules: view.settings.clone(),
                    ..GameConfig::default()
                };
                cfg.adjust(items[selected.min(items.len() - 1)], delta);
                if cfg.rules == view.settings {
                    return Reply::default();
                }
                // The room would refuse these too, without a word.
                let players = view.shell.seats.len();
                match max_players_for(&cfg.rules) {
                    Some(fit) if fit >= players => Reply::fx(GameFx::Settings(encode(&cfg.rules))),
                    _ => Reply::notice(format!("{players} snakes don't fit on that board.")),
                }
            }
            Key::Esc | Key::Enter | Key::Char('q' | 'e') => Reply::to(Next::Room),
            _ => Reply::default(),
        }
    }

    /// Picking a quick-play pool: Enter waits in it.
    fn quick_play_key(&mut self, selected: usize, key: Key) -> Reply {
        if let Some(selected) = key.step(selected, QUICK_POOLS.len()) {
            self.screen = SnakeScreen::QuickPlay { selected };
            return Reply::default();
        }
        match key {
            Key::Enter => {
                self.quick = selected;
                self.screen = SnakeScreen::Menu;
                Reply {
                    fx: vec![GameFx::SavePrefs(self.prefs())],
                    next: Next::Queue(QUICK_POOLS[selected].0),
                    ..Reply::default()
                }
            }
            Key::Esc | Key::Char('q') => {
                self.screen = SnakeScreen::Menu;
                Reply::to(Next::Home)
            }
            _ => Reply::default(),
        }
    }
}

pub(crate) fn direction(key: Key) -> Option<Direction> {
    match key {
        Key::Up | Key::Char('w') => Some(Direction::Up),
        Key::Down | Key::Char('s') => Some(Direction::Down),
        Key::Left | Key::Char('a') => Some(Direction::Left),
        Key::Right | Key::Char('d') => Some(Direction::Right),
        _ => None,
    }
}

/// What the host changed, going from the rules `old` to those in `room`.
fn rules_changed(old: &Rules, room: &RoomView) -> String {
    let host = room
        .shell
        .host()
        .map_or("The host", |h| h.who.name.as_str());
    let value = |rules: &Rules, i: usize| {
        GameConfig {
            rules: rules.clone(),
            ..GameConfig::default()
        }
        .value_str(i)
    };
    let changed: Vec<usize> = room_settings()
        .into_iter()
        .filter(|&i| value(old, i) != value(&room.settings, i))
        .collect();
    match changed[..] {
        [i] => format!(
            "{host} set {} to {}.",
            SETTINGS[i].label,
            value(&room.settings, i)
        ),
        _ => format!("{host} changed the rules."),
    }
}

impl GameUi for SnakeUi {
    fn id(&self) -> game_api::GameId {
        game_api::GameId::Snake
    }

    fn title(&self) -> &'static str {
        "Snake"
    }

    fn logo(&self) -> (&'static [&'static str], Color) {
        (&ui::LOGO, Color::LightGreen)
    }

    fn home_items(&self) -> &'static [&'static str] {
        &["Single player"]
    }

    fn open(&mut self, _: usize) {
        self.screen = SnakeScreen::Menu;
    }

    fn room_note(&self) -> Option<String> {
        Some("your settings".to_string())
    }

    fn room_settings(&self) -> Vec<u8> {
        encode(&self.cfg.rules)
    }

    /// Opens the choice of pools, at the one last played.
    fn quick_play(&mut self, _: Size) -> Result<QuickStart, String> {
        self.screen = SnakeScreen::QuickPlay {
            selected: self.quick,
        };
        Ok(QuickStart::Choose)
    }

    /// Another match waits in the same pool: the choice was made already.
    fn again_in_same_pool(&self) -> bool {
        true
    }

    fn pool_name(&self, pool: &str) -> Option<String> {
        QUICK_POOLS
            .iter()
            .find(|(id, ..)| *id == pool)
            .map(|(_, name, _)| name.to_string())
    }

    fn ratings(&self) -> &'static [Category] {
        &[
            Category {
                id: QUICK_RATING.category,
                label: "Classic",
            },
            Category {
                id: GUNS_RATING.category,
                label: "Guns",
            },
        ]
    }

    fn bots(&self) -> bool {
        true
    }

    fn key(&mut self, key: Key, ctx: &Ctx) -> Reply {
        // Snake's screens type nothing, so case never matters.
        let key = key.lower();
        let mut reply = Reply::default();
        match self.screen {
            SnakeScreen::Menu => self.menu_key(key, &mut reply),
            SnakeScreen::Playing => {
                if matches!(key, Key::Esc | Key::Char('p')) {
                    self.screen = SnakeScreen::Paused { selected: 0 };
                } else if let (Key::Char('f'), Some(game)) = (key, &mut self.game) {
                    game.fire();
                } else if let (Some(dir), Some(game)) = (direction(key), &mut self.game) {
                    game.turn(dir);
                }
            }
            SnakeScreen::Paused { selected } => match key {
                Key::Esc | Key::Char('r' | 'p') => self.screen = SnakeScreen::Playing,
                Key::Up | Key::Down | Key::Char('w' | 's' | 'k' | 'j') => {
                    self.screen = SnakeScreen::Paused {
                        selected: 1 - selected,
                    };
                }
                Key::Enter if selected == 0 => self.screen = SnakeScreen::Playing,
                Key::Enter | Key::Char('q') => self.quit_to_menu(&mut reply.fx),
                _ => {}
            },
            SnakeScreen::GameOver { .. } => match key {
                Key::Enter | Key::Esc | Key::Char(' ') => {
                    self.game = None;
                    self.screen = SnakeScreen::Menu;
                }
                Key::Char('r') => self.start_game(&mut reply),
                Key::Char('q') => {
                    self.game = None;
                    self.screen = SnakeScreen::Menu;
                    reply.next = Next::Home;
                }
                _ => {}
            },
            SnakeScreen::RoomRules { selected } => {
                reply = self.room_rules_key(selected, key, ctx.room);
            }
            SnakeScreen::QuickPlay { selected } => reply = self.quick_play_key(selected, key),
        }
        self.pause_if_hidden();
        reply
    }

    fn tick(&mut self, dt: Duration) -> Vec<GameFx> {
        let mut fx = Vec::new();
        if self.screen == SnakeScreen::Playing {
            if let Some(game) = &mut self.game {
                game.advance(dt);
            }
            if self.game.as_ref().is_some_and(|g| g.over.is_some()) {
                let new_record = self.save_record(&mut fx);
                self.screen = SnakeScreen::GameOver { new_record };
            }
        }
        fx
    }

    fn next_wake(&self) -> Option<Duration> {
        match (&self.screen, &self.game) {
            // Frozen until the first key; nothing to wake for.
            (SnakeScreen::Playing, Some(game)) if game.started => {
                Some(game.until_next_step().unwrap_or_default())
            }
            _ => None,
        }
    }

    fn resize(&mut self, size: Size) {
        self.size = size;
        self.pause_if_hidden();
    }

    fn playing(&self) -> bool {
        self.screen == SnakeScreen::Playing
    }

    fn draw(&self, frame: &mut Frame, ctx: &Ctx) {
        ui::draw(frame, self, ctx);
    }

    fn in_play(&self, room: &InRoom) -> bool {
        let Some(view) = snake_view(room) else {
            return false;
        };
        matches!(
            view.shell.phase,
            PhaseView::Countdown { .. } | PhaseView::Running { .. }
        ) && room
            .my_seat()
            .and_then(|s| racer(view, s))
            .is_some_and(|r| r.alive)
    }

    fn room_key(&mut self, key: Key, room: &InRoom) -> Option<Reply> {
        let key = key.lower();
        let view = snake_view(room)?;
        match &view.shell.phase {
            PhaseView::Waiting { .. } => match key {
                // The host makes the next round faster or slower, a step
                // at a time on the settings' grid. The room decides; its
                // next view shows the speed.
                Key::Left | Key::Right | Key::Char('a' | 'h' | 'd' | 'l') if room.hosting() => {
                    let now = view.settings.tick_ms;
                    let next = if matches!(key, Key::Left | Key::Char('a' | 'h')) {
                        now.saturating_sub(1) / TICK_MS_STEP * TICK_MS_STEP
                    } else {
                        (now / TICK_MS_STEP + 1) * TICK_MS_STEP
                    }
                    .clamp(TICK_MS_MIN, TICK_MS_MAX);
                    if next == now {
                        return Some(Reply::default());
                    }
                    let rules = Rules {
                        tick_ms: next,
                        ..view.settings.clone()
                    };
                    Some(Reply::fx(GameFx::Settings(encode(&rules))))
                }
                // Everyone can read the rules; the host changes them there.
                Key::Char('e') => {
                    self.screen = SnakeScreen::RoomRules { selected: 0 };
                    Some(Reply::to(Next::Game))
                }
                _ => None,
            },
            PhaseView::Countdown { .. } | PhaseView::Running { .. } if self.in_play(room) => {
                let input = match key {
                    Key::Char('f') => SnakeInput::Fire,
                    key => SnakeInput::Turn(direction(key)?),
                };
                Some(Reply::fx(GameFx::Play(encode(&input))))
            }
            _ => None,
        }
    }

    fn settings_note(&self, old: &dyn AnyView, room: &InRoom) -> Option<String> {
        let (old, now) = (old.of::<Snake>()?, snake_view(room)?);
        (old.settings != now.settings).then(|| rules_changed(&old.settings, now))
    }

    fn lobby_line(&self, room: &InRoom) -> Vec<Span<'static>> {
        let Some(view) = snake_view(room) else {
            return Vec::new();
        };
        let rules = &view.settings;
        let dim = shell_tui::ui::dim();
        // The host can change the speed, so theirs stands out, with arrows.
        let tick = format!("{} ms ticks", rules.tick_ms);
        let tick = if room.hosting() {
            Span::styled(
                format!("◂ {tick} ▸"),
                ratatui::style::Style::default().fg(Color::LightYellow),
            )
        } else {
            Span::styled(tick, dim)
        };
        let mut line = vec![
            Span::styled(format!("{}×{} board · ", rules.width, rules.height), dim),
            tick,
        ];
        if rules.guns {
            line.push(Span::styled(" · guns", dim));
        }
        line
    }

    fn lobby_keys(&self) -> Option<&'static str> {
        Some("e rules")
    }

    fn host_keys(&self) -> Option<&'static str> {
        Some("←→ speed")
    }

    fn draw_round(&self, frame: &mut Frame, room: &InRoom, ctx: &Ctx) {
        let _ = ctx;
        ui::draw_round(frame, self, room);
    }

    fn result_note(&self, room: &InRoom) -> Option<String> {
        snake_view(room)?
            .game
            .time_up
            .then(|| "Time's up".to_string())
    }

    fn score(&self, room: &InRoom, who: PlayerId) -> String {
        let length = snake_view(room)
            .and_then(|v| v.game.racer(who))
            .map_or(0, |r| r.length);
        length.to_string()
    }

    fn restore(&mut self, saved: Saved) {
        self.cfg = GameConfig::from_saved_text(&saved.prefs);
        self.quick = saved
            .prefs
            .lines()
            .find_map(|line| line.strip_prefix(QUICK_PREF))
            .and_then(|pool| QUICK_POOLS.iter().position(|(id, ..)| *id == pool.trim()))
            .unwrap_or_default();
        self.records = saved.records;
    }

    fn saved(&self) -> Saved {
        Saved {
            prefs: self.prefs(),
            records: self.records.clone(),
        }
    }

    fn quit(&mut self) -> Vec<GameFx> {
        let mut fx = Vec::new();
        self.save_record(&mut fx);
        fx
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snake() -> SnakeUi {
        let mut ui = SnakeUi::default();
        ui.resize(Size::new(120, 40));
        ui
    }

    fn key(ui: &mut SnakeUi, key: Key) -> Reply {
        ui.key(key, &Ctx::alone(ui.size))
    }

    #[test]
    fn menu_d_adjusts_and_r_twice_resets() {
        let mut ui = snake();
        key(&mut ui, Key::Char('d'));
        key(&mut ui, Key::Char('D'));
        assert_eq!(ui.cfg.rules.width, 32);
        key(&mut ui, Key::Char('r'));
        assert!(ui.confirm_reset);
        key(&mut ui, Key::Char('x'));
        assert_eq!(ui.cfg.rules.width, 32, "any other key cancels the reset");
        key(&mut ui, Key::Char('r'));
        key(&mut ui, Key::Char('r'));
        assert_eq!(ui.cfg, GameConfig::default());
    }

    #[test]
    fn quitting_keeps_a_new_record() {
        let mut ui = snake();
        assert_eq!(ui.quit(), [], "nothing played yet");
        let reply = key(&mut ui, Key::Enter);
        assert_eq!(
            reply.fx,
            [GameFx::SavePrefs(GameConfig::default().to_saved_text())]
        );
        assert_eq!(ui.screen, SnakeScreen::Playing);
        // The first game on these rules is a new record, saved on the way
        // out.
        let len = ui.game.as_ref().unwrap().len() as u32;
        let key = GameConfig::default().rules.record_key();
        assert_eq!(ui.quit(), [GameFx::Record { key, best: len }]);
    }

    #[test]
    fn pauses_when_the_board_stops_fitting() {
        let mut ui = snake();
        key(&mut ui, Key::Enter);
        assert_eq!(ui.screen, SnakeScreen::Playing);
        ui.resize(Size::new(40, 15));
        assert_eq!(ui.screen, SnakeScreen::Paused { selected: 0 });
        key(&mut ui, Key::Esc);
        assert_eq!(
            ui.screen,
            SnakeScreen::Paused { selected: 0 },
            "can't resume yet"
        );
        ui.resize(Size::new(120, 40));
        key(&mut ui, Key::Esc);
        assert_eq!(ui.screen, SnakeScreen::Playing);
    }

    #[test]
    fn the_settings_and_a_finished_game_go_home_on_q() {
        let mut ui = snake();
        assert_eq!(key(&mut ui, Key::Char('q')).next, Next::Home);
        key(&mut ui, Key::Enter);
        ui.game.as_mut().unwrap().over = Some(snake_core::solo::GameOverReason::HitWall);
        ui.tick(Duration::from_millis(1));
        assert!(matches!(ui.screen, SnakeScreen::GameOver { .. }));
        assert_eq!(key(&mut ui, Key::Char('q')).next, Next::Home);
        assert_eq!(ui.screen, SnakeScreen::Menu);
    }

    /// The settings menu's row for `key`.
    fn row(key: &str) -> usize {
        SETTINGS.iter().position(|s| s.key == key).unwrap()
    }

    #[test]
    fn guns_in_single_player_bring_a_bot() {
        let mut ui = snake();
        ui.selected = row("guns");
        key(&mut ui, Key::Right);
        assert!(ui.cfg.rules.guns);
        assert_eq!(ui.cfg.rules.bots, 1, "someone to shoot");
        ui.selected = row("bots");
        key(&mut ui, Key::Left);
        assert_eq!(ui.cfg.rules.bots, 1, "not none while guns are on");
        ui.selected = row("guns");
        key(&mut ui, Key::Left);
        ui.selected = row("bots");
        key(&mut ui, Key::Left);
        assert_eq!(ui.cfg.rules.bots, 0);
    }

    #[test]
    fn f_fires_in_single_player() {
        use snake_core::guns::{GunKind, Held};

        let mut ui = snake();
        ui.selected = row("guns");
        key(&mut ui, Key::Right);
        key(&mut ui, Key::Enter);
        let game = ui.game.as_mut().unwrap();
        assert_eq!(game.world.snakes.len(), 2, "you and a bot");
        game.world.snakes[0].gun = Some(Held {
            kind: GunKind::Laser,
            until: Duration::from_secs(60),
        });
        // Going on the way it faces starts the game.
        let ahead = match game.snake().dir {
            Direction::Up => Key::Up,
            Direction::Down => Key::Down,
            Direction::Left => Key::Left,
            Direction::Right => Key::Right,
        };
        let tick = game.tick_interval();
        key(&mut ui, ahead);
        key(&mut ui, Key::Char('f'));
        ui.tick(tick);
        let game = ui.game.as_ref().unwrap();
        assert_eq!(game.world.beams.len(), 1, "the laser fired");
        assert_eq!(game.snake().gun, None);
    }

    #[test]
    fn bots_that_dont_fit_say_so() {
        let mut ui = snake();
        ui.cfg.rules = Rules {
            width: 10,
            height: 8,
            bots: 7,
            ..Rules::default()
        };
        let reply = key(&mut ui, Key::Enter);
        assert!(
            reply
                .notice
                .as_deref()
                .is_some_and(|n| n.contains("7 bots")),
            "{:?}",
            reply.notice
        );
        assert_eq!(ui.screen, SnakeScreen::Menu);
    }
}

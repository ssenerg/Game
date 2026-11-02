//! The board and everything on it: walls, food, guns, and any number of
//! snakes that all move one cell at the same moment.

use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use rand::rngs::Xoshiro256PlusPlus;
use rand::{RngExt, SeedableRng};

use crate::board;
use crate::config::Rules;
use crate::geom::{Direction, Point, wrap_next};
use crate::guns::{
    BEAM_SHOWN, Beam, Bullet, Cloud, Held, Hit, Item, MARK_SPEED, Pickup, Throw, disc, next_cell,
};
use crate::snake::{DeathCause, Snake};
use crate::spawn;
use crate::view::{MatchView, SnakeView};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum FoodKind {
    Normal,
    Super,
    Poison,
}

impl FoodKind {
    pub const ALL: [FoodKind; 3] = [FoodKind::Normal, FoodKind::Super, FoodKind::Poison];
}

fn spawn_interval_s(rules: &Rules, kind: FoodKind) -> u64 {
    match kind {
        FoodKind::Normal => rules.food_interval_s,
        FoodKind::Super => rules.super_interval_s,
        FoodKind::Poison => rules.poison_interval_s,
    }
}

fn ttl_s(rules: &Rules, kind: FoodKind) -> u64 {
    match kind {
        FoodKind::Normal => rules.food_ttl_s,
        FoodKind::Super => rules.super_ttl_s,
        FoodKind::Poison => rules.poison_ttl_s,
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Food {
    pub pos: Point,
    pub kind: FoodKind,
    pub expires_at: Option<Duration>,
}

/// What happened during one [`World::step`], in snake order.
#[derive(Default, Debug, PartialEq, Eq)]
pub struct StepOutcome {
    pub deaths: Vec<(u8, DeathCause)>,
    pub eaten: Vec<(u8, FoodKind)>,
    /// Shots that hit, in the order their snakes come.
    pub hits: Vec<Hit>,
}

/// One living snake's move during a step.
struct Move {
    snake: usize,
    next: Point,
    /// How many of its current cells stay part of the body.
    keep: usize,
    death: Option<DeathCause>,
}

#[derive(Clone)]
pub struct World {
    pub rules: Rules,
    pub width: u16,
    pub height: u16,
    walls: Arc<[bool]>,
    pub snakes: Vec<Snake>,
    pub foods: Vec<Food>,
    pub items: Vec<Pickup>,
    pub bullets: Vec<Bullet>,
    pub beams: Vec<Beam>,
    /// Grenades' blasts and flashbangs' flashes, kept a moment to be seen.
    pub bursts: Vec<(Item, Cloud)>,
    pub smoke: Vec<Cloud>,
    /// Game time. It only moves through [`Self::advance_clock`], so a world
    /// driven by fixed ticks replays exactly from its seed and inputs.
    pub clock: Duration,
    next_spawn: [Option<Duration>; FoodKind::ALL.len()],
    /// When the next gun appears; never in a game alone.
    next_gun: Option<Duration>,
    /// When the next thrown item does.
    next_thrown: Option<Duration>,
    free_cells: usize,
    /// Whether dead snakes turn into food. Otherwise they stay where they
    /// died, as the single-player game-over screen shows.
    pub corpses_to_food: bool,
    /// A snake that stays where it died even so: the player's, in single
    /// player against bots.
    pub keeps_body: Option<u8>,
    /// Whether each food spawn drops one item per two living snakes, rather
    /// than one.
    pub scale_food: bool,
    rng: Xoshiro256PlusPlus,
    /// Scratch space for [`Self::step`]: snake segments per cell.
    occupancy: Vec<u16>,
}

impl World {
    /// A board with one snake placed at random, for single player.
    pub fn solo(rules: Rules, seed: u64) -> Result<Self> {
        let mut rng = Xoshiro256PlusPlus::seed_from_u64(seed);
        let walls = board::generate_walls(&rules, &mut rng);
        let (body, dir) = board::place_snake(&walls, rules.width, rules.height, &mut rng)?;
        Ok(Self::new(rules, walls, vec![Snake::new(0, body, dir)], rng))
    }

    /// A board with `players` snakes spread apart, for multiplayer. Snakes
    /// that die turn into food, food comes faster with more players, and
    /// guns appear if the rules have them.
    pub fn with_players(rules: Rules, seed: u64, players: usize) -> Result<Self> {
        let mut rng = Xoshiro256PlusPlus::seed_from_u64(seed);
        let walls = board::generate_walls(&rules, &mut rng);
        let snakes = spawn::spawn_points(&walls, rules.width, rules.height, players, &mut rng)?
            .into_iter()
            .enumerate()
            .map(|(i, (body, dir))| Snake::new(i as u8, body, dir))
            .collect();
        let mut world = Self::new(rules, walls, snakes, rng);
        world.corpses_to_food = true;
        world.scale_food = true;
        let rules = &world.rules;
        world.next_gun = rules
            .guns
            .then(|| Duration::from_secs(rules.gun_interval_s));
        world.next_thrown = (rules.guns && rules.thrown_interval_s > 0)
            .then(|| Duration::from_secs(rules.thrown_interval_s));
        Ok(world)
    }

    fn new(rules: Rules, walls: Vec<bool>, snakes: Vec<Snake>, rng: Xoshiro256PlusPlus) -> Self {
        let free_cells = walls.iter().filter(|w| !**w).count();
        let next_spawn = FoodKind::ALL.map(|kind| {
            let interval = spawn_interval_s(&rules, kind);
            let first = if kind == FoodKind::Normal {
                1
            } else {
                interval
            };
            (interval > 0).then(|| Duration::from_secs(first))
        });
        Self {
            width: rules.width,
            height: rules.height,
            occupancy: vec![0; walls.len()],
            walls: walls.into(),
            rules,
            snakes,
            foods: Vec::new(),
            items: Vec::new(),
            bullets: Vec::new(),
            beams: Vec::new(),
            bursts: Vec::new(),
            smoke: Vec::new(),
            clock: Duration::ZERO,
            next_spawn,
            next_gun: None,
            next_thrown: None,
            free_cells,
            corpses_to_food: false,
            keeps_body: None,
            scale_food: false,
            rng,
        }
    }

    fn idx(&self, p: Point) -> usize {
        p.y as usize * self.width as usize + p.x as usize
    }

    pub fn wall_at(&self, x: u16, y: u16) -> bool {
        self.walls[self.idx(Point { x, y })]
    }

    /// How many cells aren't walls.
    pub fn free_cells(&self) -> usize {
        self.free_cells
    }

    /// Queues a turn for snake `id`'s upcoming ticks. Returns whether it was
    /// kept; keys for dead or unknown snakes are dropped.
    pub fn steer(&mut self, id: u8, dir: Direction) -> bool {
        let (w, h) = (self.width, self.height);
        self.snakes
            .iter_mut()
            .find(|s| s.id == id)
            .is_some_and(|s| s.enqueue(dir, w, h))
    }

    /// Snake `id`'s `f`: it fires the gun it holds on the next step, or
    /// starts throwing the thrown item it holds, or, already throwing,
    /// throws it where the mark is, as letting go of `f` does. Returns
    /// whether it did any of these.
    pub fn fire(&mut self, id: u8) -> bool {
        let Some(snake) = self.snakes.iter_mut().find(|s| s.id == id && s.alive()) else {
            return false;
        };
        if let Some(throw) = snake.throw.as_mut()
            && !throw.released
        {
            throw.released = true;
            return true;
        }
        match snake.held {
            Some(held) if held.kind.thrown() => {
                snake.held = None;
                snake.throw = Some(Throw {
                    kind: held.kind,
                    dir: snake.dir,
                    mark: None,
                    travelled: 0,
                    released: false,
                });
            }
            Some(_) => snake.firing = true,
            None => return false,
        }
        true
    }

    /// Snake `id` let go of `f`: a throw on its way lands where its mark
    /// is, on the next step. Returns whether there was one.
    pub fn release(&mut self, id: u8) -> bool {
        let throw = self
            .snakes
            .iter_mut()
            .find(|s| s.id == id && s.alive())
            .and_then(|s| s.throw.as_mut());
        match throw {
            Some(throw) if !throw.released => {
                throw.released = true;
                true
            }
            _ => false,
        }
    }

    /// Moves game time forward: food and guns that have run out disappear,
    /// and new ones appear on schedule.
    pub fn advance_clock(&mut self, dt: Duration) {
        self.clock += dt;
        let clock = self.clock;
        self.foods
            .retain(|f| f.expires_at.is_none_or(|e| e > clock));
        self.items.retain(|g| g.expires_at > clock);
        self.beams.retain(|b| b.until > clock);
        self.bursts.retain(|(_, b)| b.until > clock);
        self.smoke.retain(|c| c.until > clock);
        for snake in &mut self.snakes {
            if snake.held.is_some_and(|g| g.until <= clock) {
                snake.held = None;
                snake.firing = false;
            }
        }
        while let Some(t) = self.next_gun {
            if clock < t {
                break;
            }
            self.spawn_item(&Item::GUNS, self.rules.gun_ttl_s);
            self.next_gun = Some(t + Duration::from_secs(self.rules.gun_interval_s));
        }
        while let Some(t) = self.next_thrown {
            if clock < t {
                break;
            }
            self.spawn_item(&Item::THROWN, self.rules.thrown_ttl_s);
            self.next_thrown = Some(t + Duration::from_secs(self.rules.thrown_interval_s));
        }

        let alive = self.snakes.iter().filter(|s| s.alive()).count();
        for (i, kind) in FoodKind::ALL.into_iter().enumerate() {
            let interval = spawn_interval_s(&self.rules, kind);
            let count = if self.scale_food && kind == FoodKind::Normal {
                alive.div_ceil(2).max(1)
            } else {
                1
            };
            while let Some(t) = self.next_spawn[i] {
                if clock < t {
                    break;
                }
                for _ in 0..count {
                    self.spawn_food(kind);
                }
                self.next_spawn[i] = Some(t + Duration::from_secs(interval));
            }
        }
    }

    /// Takes snake `id` out of the game, as if it had died, because its
    /// player left. Returns whether it was still alive.
    pub fn remove(&mut self, id: u8) -> bool {
        let Some(i) = self.snakes.iter().position(|s| s.id == id && s.alive()) else {
            return false;
        };
        self.snakes[i].death = Some(DeathCause::Left);
        if self.corpses_to_food {
            self.turn_into_food(i);
        }
        true
    }

    pub fn alive(&self) -> usize {
        self.snakes.iter().filter(|s| s.alive()).count()
    }

    /// Moves every living snake one cell, all at once.
    ///
    /// Each snake first takes its next queued turn. Then, judged against
    /// everyone's positions after the move, a snake dies when its head:
    /// - hits a wall;
    /// - lands on another snake's new head or body, or swaps places with
    ///   another head (so a head-on crash kills both);
    /// - lands on its own new body, unless pass-through-self is on;
    /// - eats poison.
    ///
    /// A tail that moves away this step frees its cell; a growing one doesn't.
    /// Survivors and poisoned snakes move; the rest die where they are. Then
    /// surviving heads eat what they landed on, and last come the guns: see
    /// [`Self::shoot`]. The order of `snakes` never changes the outcome.
    pub fn step(&mut self) -> StepOutcome {
        let (w, h) = (self.width, self.height);
        let wu = w as usize;
        let at = |p: Point| p.y as usize * wu + p.x as usize;
        let mut out = StepOutcome::default();

        let mut moves = Vec::new();
        for (i, snake) in self.snakes.iter_mut().enumerate() {
            if !snake.alive() {
                continue;
            }
            snake.take_turn(w, h);
            let next = wrap_next(snake.head(), snake.dir, w, h);
            let keep = snake.len() - usize::from(!snake.tick_growth());
            moves.push(Move {
                snake: i,
                next,
                keep,
                death: None,
            });
        }

        // Segments per cell once everyone has moved.
        self.occupancy.fill(0);
        for m in &moves {
            self.occupancy[at(m.next)] += 1;
            for &p in self.snakes[m.snake].body.iter().take(m.keep) {
                self.occupancy[at(p)] += 1;
            }
        }

        for k in 0..moves.len() {
            let m = &moves[k];
            let snake = &self.snakes[m.snake];
            let own = snake
                .body
                .iter()
                .take(m.keep)
                .filter(|&&p| p == m.next)
                .count();
            let others = usize::from(self.occupancy[at(m.next)]) - 1 - own;
            // One-cell snakes leave nothing behind to collide with, so check
            // swaps explicitly.
            let swapped = moves.iter().any(|o| {
                o.snake != m.snake
                    && o.next == snake.head()
                    && m.next == self.snakes[o.snake].head()
            });
            let poison = self
                .foods
                .iter()
                .any(|f| f.pos == m.next && f.kind == FoodKind::Poison);
            moves[k].death = if self.walls[at(m.next)] {
                Some(DeathCause::Wall)
            } else if others > 0 || swapped {
                Some(DeathCause::OtherSnake)
            } else if own > 0 && !self.rules.pass_through_self {
                Some(DeathCause::OwnBody)
            } else if poison {
                Some(DeathCause::Poison)
            } else {
                None
            };
        }

        for m in &moves {
            let snake = &mut self.snakes[m.snake];
            if matches!(m.death, None | Some(DeathCause::Poison)) {
                snake.body.push_front(m.next);
                snake.body.truncate(m.keep + 1);
            }
            if let Some(cause) = m.death {
                snake.death = Some(cause);
                out.deaths.push((snake.id, cause));
            }
        }

        for m in &moves {
            let Some(i) = self.foods.iter().position(|f| f.pos == m.next) else {
                continue;
            };
            match m.death {
                None => {
                    let food = self.foods.remove(i);
                    let n = match food.kind {
                        FoodKind::Super => self.rules.super_growth,
                        _ => 1,
                    };
                    let snake = &mut self.snakes[m.snake];
                    snake.grow(n, self.rules.growth_mode);
                    out.eaten.push((snake.id, food.kind));
                }
                Some(DeathCause::Poison) => {
                    self.foods.remove(i);
                }
                Some(_) => {}
            }
        }

        if self.corpses_to_food {
            for m in &moves {
                if m.death.is_some() {
                    self.turn_into_food(m.snake);
                }
            }
        }
        self.shoot(&mut out);
        out
    }

    /// The items' part of a step, once every snake has moved:
    /// - bullets in flight go on, then the shots asked for fire, from the
    ///   heads where they now are, and throws move or land;
    /// - every hit lands at once, from shots and blasts: a snake hit in the
    ///   head dies, and one hit in the body loses everything behind the hit
    ///   nearest its head, the piece turning into food;
    /// - last, the heads left take any item they're on, in place of one
    ///   they hold.
    fn shoot(&mut self, out: &mut StepOutcome) {
        // (shooter, target snake's place in `snakes`, cell of its body hit)
        let mut hits: Vec<(u8, usize, usize)> = Vec::new();
        let bullets = std::mem::take(&mut self.bullets);
        for bullet in bullets {
            self.fly(bullet, true, &mut hits);
        }
        for i in 0..self.snakes.len() {
            let snake = &mut self.snakes[i];
            let firing = std::mem::take(&mut snake.firing);
            if !firing || !snake.alive() {
                continue;
            }
            let Some(gun) = snake.held.take() else {
                continue;
            };
            let (owner, pos, dir) = (snake.id, snake.head(), snake.dir);
            match gun.kind {
                Item::Laser => {
                    let cells = self.line_of_fire(pos, dir, usize::MAX);
                    for &p in &cells {
                        hits.extend(self.struck(p, owner).map(|(t, at)| (owner, t, at)));
                    }
                    self.beams.push(Beam {
                        dir,
                        cells,
                        until: self.clock + BEAM_SHOWN,
                    });
                }
                Item::Pistol => self.fly(Bullet { owner, pos, dir }, false, &mut hits),
                // Thrown, never fired: see `Self::fire`.
                Item::Grenade | Item::Flashbang | Item::Smoke => {}
            }
        }
        self.throws(&mut hits);

        let mut order: Vec<usize> = hits.iter().map(|h| h.1).collect();
        order.sort_unstable();
        order.dedup();
        for target in order {
            let &(shooter, _, keep) = hits
                .iter()
                .filter(|h| h.1 == target)
                .min_by_key(|h| h.2)
                .expect("a hit on it");
            let snake = &mut self.snakes[target];
            out.hits.push(Hit {
                shooter,
                target: snake.id,
                left: keep,
            });
            if keep == 0 {
                snake.death = Some(DeathCause::Shot);
                snake.held = None;
                out.deaths.push((snake.id, DeathCause::Shot));
                if self.corpses_to_food {
                    self.turn_into_food(target);
                }
            } else {
                let piece = snake.cut(keep);
                if self.corpses_to_food {
                    self.cells_to_food(piece);
                }
            }
        }

        let hold = Duration::from_secs(self.rules.gun_hold_s);
        for snake in self.snakes.iter_mut().filter(|s| s.alive()) {
            if let Some(i) = self.items.iter().position(|g| g.pos == snake.head()) {
                let gun = self.items.remove(i);
                snake.held = Some(Held {
                    kind: gun.kind,
                    until: self.clock + hold,
                });
            }
        }
    }

    /// Throws on their way: one let go lands where its mark is; the rest
    /// move their marks on, and one whose mark would go past the board's
    /// edge is lost. A mark sets off from the head, the step after its
    /// throw started. Landing blasts go in `hits`, to land with the shots.
    fn throws(&mut self, hits: &mut Vec<(u8, usize, usize)>) {
        let (w, h) = (self.width, self.height);
        for i in 0..self.snakes.len() {
            let Some(mut throw) = self.snakes[i].throw.take() else {
                continue;
            };
            let snake = &self.snakes[i];
            if !snake.alive() {
                continue;
            }
            let (owner, head) = (snake.id, snake.head());
            match throw.mark {
                Some(mark) if throw.released => self.land(owner, throw.kind, mark, hits),
                None if throw.released => self.land(owner, throw.kind, head, hits),
                None => {
                    throw.mark = Some(head);
                    self.snakes[i].throw = Some(throw);
                }
                Some(mark) => {
                    let mut next = Some(mark);
                    for _ in 0..MARK_SPEED {
                        next = next.and_then(|p| next_cell(p, throw.dir, w, h));
                    }
                    if next.is_some() {
                        throw.mark = next;
                        throw.travelled += MARK_SPEED;
                        self.snakes[i].throw = Some(throw);
                    }
                }
            }
        }
    }

    /// A thrown item of `owner`'s, landing at `at`.
    fn land(&mut self, owner: u8, kind: Item, at: Point, hits: &mut Vec<(u8, usize, usize)>) {
        let (w, h, until) = (self.width, self.height, self.clock + BEAM_SHOWN);
        match kind {
            Item::Grenade => {
                let cells = disc(at, self.rules.grenade_radius, w, h);
                for &p in &cells {
                    hits.extend(self.struck(p, owner).map(|(t, at)| (owner, t, at)));
                }
                self.bursts.push((kind, Cloud { cells, until }));
            }
            Item::Flashbang => {
                let blind_until = self.clock + Duration::from_secs(self.rules.flash_s);
                for snake in self
                    .snakes
                    .iter_mut()
                    .filter(|s| s.alive() && s.id != owner)
                {
                    snake.blind_until = blind_until;
                    snake.plan = None;
                }
                let cells = disc(at, 1, w, h);
                self.bursts.push((kind, Cloud { cells, until }));
            }
            Item::Smoke => {
                let cells = disc(at, self.rules.smoke_radius, w, h);
                let until = self.clock + Duration::from_secs(self.rules.smoke_s);
                self.smoke.push(Cloud { cells, until });
            }
            // Fired, never thrown.
            Item::Laser | Item::Pistol => {}
        }
    }

    /// A number from the world's own random numbers, so a seed replays.
    pub(crate) fn roll(&mut self, range: std::ops::RangeInclusive<usize>) -> usize {
        self.rng.random_range(range)
    }

    /// Whether smoke hides the cell at `p`.
    pub fn smoky(&self, p: Point) -> bool {
        self.smoke.iter().any(|c| c.cells.contains(&p))
    }

    /// Moves a bullet up to the rules' bullet speed in cells, checking first the cell
    /// it's on if `here`, since a head may have moved into it. It stops at a
    /// wall, at the board's edge, or at the first snake it hits; otherwise
    /// it flies on next step.
    fn fly(&mut self, mut bullet: Bullet, here: bool, hits: &mut Vec<(u8, usize, usize)>) {
        let speed = self.rules.bullet_speed.max(1) as usize;
        let path = self.line_of_fire(bullet.pos, bullet.dir, speed);
        let checked = here
            .then_some(bullet.pos)
            .into_iter()
            .chain(path.iter().copied());
        for p in checked {
            if let Some((target, at)) = self.struck(p, bullet.owner).next() {
                hits.push((bullet.owner, target, at));
                return;
            }
        }
        if path.len() < speed {
            // It reached a wall or the edge.
            return;
        }
        bullet.pos = *path.last().expect("a full path");
        self.bullets.push(bullet);
    }

    /// Up to `most` cells from `from` straight on in `dir`, up to the first
    /// wall or the board's edge, whichever comes first.
    pub(crate) fn line_of_fire(&self, from: Point, dir: Direction, most: usize) -> Vec<Point> {
        let (w, h) = (self.width, self.height);
        let mut cells = Vec::new();
        let mut p = from;
        while cells.len() < most
            && let Some(next) = next_cell(p, dir, w, h)
            && !self.walls[self.idx(next)]
        {
            cells.push(next);
            p = next;
        }
        cells
    }

    /// The living snakes, other than `owner`, with a cell at `p`: each one's
    /// place in `snakes`, and the cell of its body nearest its head there.
    fn struck(&self, p: Point, owner: u8) -> impl Iterator<Item = (usize, usize)> + '_ {
        self.snakes.iter().enumerate().filter_map(move |(i, s)| {
            if !s.alive() || s.id == owner {
                return None;
            }
            s.body.iter().position(|&c| c == p).map(|at| (i, at))
        })
    }

    /// Replaces a dead snake's body with food, except where something else is.
    fn turn_into_food(&mut self, snake: usize) {
        if self.keeps_body == Some(self.snakes[snake].id) {
            return;
        }
        let body = std::mem::take(&mut self.snakes[snake].body);
        self.cells_to_food(body);
    }

    /// Puts food on `cells`, except where something else is.
    fn cells_to_food(&mut self, cells: impl IntoIterator<Item = Point>) {
        for p in cells {
            let taken = self.walls[self.idx(p)]
                || self.foods.iter().any(|f| f.pos == p)
                || self.items.iter().any(|g| g.pos == p)
                || self.snakes.iter().any(|s| s.alive() && s.body.contains(&p));
            if !taken {
                self.place_food(p, FoodKind::Normal);
            }
        }
    }

    /// A cell picked at random with no wall, snake, food or gun on it.
    fn free_cell(&mut self) -> Option<Point> {
        let mut taken: Vec<bool> = self.walls.to_vec();
        for p in self.snakes.iter().flat_map(|s| &s.body) {
            taken[self.idx(*p)] = true;
        }
        for p in self
            .foods
            .iter()
            .map(|f| f.pos)
            .chain(self.items.iter().map(|g| g.pos))
        {
            taken[self.idx(p)] = true;
        }
        let w = self.width as usize;
        let cells: Vec<Point> = (0..taken.len())
            .filter(|&i| !taken[i])
            .map(|i| Point {
                x: (i % w) as u16,
                y: (i / w) as u16,
            })
            .collect();
        if cells.is_empty() {
            return None;
        }
        Some(cells[self.rng.random_range(0..cells.len())])
    }

    fn spawn_food(&mut self, kind: FoodKind) {
        if let Some(pos) = self.free_cell() {
            self.place_food(pos, kind);
        }
    }

    /// One of `kinds` at random, on a free cell, for `ttl_s` seconds.
    fn spawn_item(&mut self, kinds: &[Item], ttl_s: u64) {
        let Some(pos) = self.free_cell() else {
            return;
        };
        let kind = kinds[self.rng.random_range(0..kinds.len())];
        self.items.push(Pickup {
            pos,
            kind,
            expires_at: self.clock + Duration::from_secs(ttl_s),
        });
    }

    fn place_food(&mut self, pos: Point, kind: FoodKind) {
        let ttl = ttl_s(&self.rules, kind);
        let expires_at = (ttl > 0).then(|| self.clock + Duration::from_secs(ttl));
        self.foods.push(Food {
            pos,
            kind,
            expires_at,
        });
    }

    pub fn view(&self) -> MatchView {
        MatchView {
            width: self.width,
            height: self.height,
            walls: Arc::clone(&self.walls),
            snakes: self
                .snakes
                .iter()
                .map(|s| SnakeView {
                    id: s.id,
                    body: s.body.iter().copied().collect(),
                    alive: s.alive(),
                    held: s
                        .held
                        .filter(|_| s.alive())
                        .map(|g| (g.kind, g.until.saturating_sub(self.clock))),
                    throwing: s
                        .throw
                        .as_ref()
                        .filter(|t| s.alive() && !t.released)
                        .map(|t| t.kind),
                    blind: (s.alive() && s.blind(self.clock)).then(|| s.blind_until - self.clock),
                })
                .collect(),
            foods: self.foods.iter().map(|f| (f.pos, f.kind)).collect(),
            items: self.items.iter().map(|g| (g.pos, g.kind)).collect(),
            bullets: self.bullets.iter().map(|b| (b.pos, b.dir)).collect(),
            beams: self
                .beams
                .iter()
                .map(|b| (b.dir, b.cells.clone()))
                .collect(),
            marks: self
                .snakes
                .iter()
                .filter(|s| s.alive())
                .filter_map(|s| {
                    let throw = s.throw.as_ref()?;
                    Some((throw.mark?, throw.kind))
                })
                .collect(),
            bursts: self
                .bursts
                .iter()
                .map(|(kind, b)| (*kind, b.cells.clone()))
                .collect(),
            smoke: {
                let mut cells: Vec<Point> = self
                    .smoke
                    .iter()
                    .flat_map(|c| c.cells.iter().copied())
                    .collect();
                cells.sort_by_key(|p| (p.y, p.x));
                cells.dedup();
                cells
            },
            clock: self.clock,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::{HashMap, VecDeque};

    use proptest::prelude::*;

    use super::*;
    use crate::config::{GrowthMode, InnerWalls, WallMode};
    use crate::geom::Direction::{self, Down, Left, Right, Up};
    use crate::guns::{Bullet, Pickup};

    fn rules(wall_mode: WallMode) -> Rules {
        Rules {
            wall_mode,
            inner_walls: InnerWalls::Off,
            ..Rules::default()
        }
    }

    /// A world with no food scheduled and the given snakes, ids in order.
    fn world(rules: Rules, snakes: &[(&[(u16, u16)], Direction)]) -> World {
        let mut rng = Xoshiro256PlusPlus::seed_from_u64(0);
        let walls = board::generate_walls(&rules, &mut rng);
        let snakes = snakes
            .iter()
            .enumerate()
            .map(|(i, (cells, dir))| {
                let body: VecDeque<Point> = cells.iter().map(|&(x, y)| Point { x, y }).collect();
                Snake::new(i as u8, body, *dir)
            })
            .collect();
        let mut world = World::new(rules, walls, snakes, rng);
        world.next_spawn = [None; 3];
        world
    }

    fn body(world: &World, id: usize) -> Vec<(u16, u16)> {
        world.snakes[id].body.iter().map(|p| (p.x, p.y)).collect()
    }

    fn deaths(world: &mut World) -> Vec<(u8, DeathCause)> {
        world.step().deaths
    }

    use DeathCause::{OtherSnake, OwnBody, Poison, Wall};

    #[test]
    fn head_on_into_the_same_cell_kills_both() {
        let mut w = world(
            rules(WallMode::Open),
            &[
                (&[(5, 5), (4, 5), (3, 5)], Right),
                (&[(7, 5), (8, 5), (9, 5)], Left),
            ],
        );
        assert_eq!(deaths(&mut w), [(0, OtherSnake), (1, OtherSnake)]);
        assert_eq!(
            body(&w, 0),
            [(5, 5), (4, 5), (3, 5)],
            "dead snakes don't move"
        );
    }

    #[test]
    fn swapping_heads_kills_both_at_any_length() {
        let mut short = world(
            rules(WallMode::Open),
            &[(&[(5, 5)], Right), (&[(6, 5)], Left)],
        );
        assert_eq!(deaths(&mut short), [(0, OtherSnake), (1, OtherSnake)]);

        let mut long = world(
            rules(WallMode::Open),
            &[(&[(5, 5), (4, 5)], Right), (&[(6, 5), (7, 5)], Left)],
        );
        assert_eq!(deaths(&mut long), [(0, OtherSnake), (1, OtherSnake)]);
    }

    #[test]
    fn running_into_a_body_kills_only_the_runner() {
        let mut w = world(
            rules(WallMode::Open),
            &[(&[(5, 5), (4, 5)], Right), (&[(6, 4), (6, 5), (6, 6)], Up)],
        );
        assert_eq!(deaths(&mut w), [(0, OtherSnake)]);
        assert_eq!(body(&w, 1), [(6, 3), (6, 4), (6, 5)]);
    }

    #[test]
    fn following_a_tail_that_moves_away_is_safe() {
        let tail_chase: [(&[(u16, u16)], Direction); 2] =
            [(&[(5, 5), (4, 5)], Right), (&[(7, 4), (7, 5), (6, 5)], Up)];
        let mut w = world(rules(WallMode::Open), &tail_chase);
        assert_eq!(deaths(&mut w), []);
        assert_eq!(body(&w, 0), [(6, 5), (5, 5)]);
    }

    #[test]
    fn a_growing_tail_stays_and_kills() {
        // Instant growth leaves the tail cell doubled for a step.
        let mut instant = world(
            rules(WallMode::Open),
            &[
                (&[(5, 5), (4, 5)], Right),
                (&[(7, 4), (7, 5), (6, 5), (6, 5)], Up),
            ],
        );
        assert_eq!(deaths(&mut instant), [(0, OtherSnake)]);

        // At-tail growth that comes due this step keeps the tail too.
        let at_tail = Rules {
            growth_mode: GrowthMode::AtTail,
            ..rules(WallMode::Open)
        };
        let mut w = world(
            at_tail,
            &[(&[(5, 5), (4, 5)], Right), (&[(7, 4), (7, 5), (6, 5)], Up)],
        );
        w.snakes[1].grow(1, GrowthMode::AtTail);
        w.snakes[1].tick_growth();
        w.snakes[1].tick_growth();
        assert_eq!(deaths(&mut w), [(0, OtherSnake)]);
    }

    #[test]
    fn own_tail_follows_the_same_rule() {
        let square: &[(u16, u16)] = &[(5, 5), (5, 6), (6, 6), (6, 5)];
        let mut w = world(rules(WallMode::Open), &[(square, Right)]);
        assert_eq!(deaths(&mut w), [], "the tail moves out of the way");

        let grown: &[(u16, u16)] = &[(5, 5), (5, 6), (6, 6), (6, 5), (6, 5)];
        let mut w = world(rules(WallMode::Open), &[(grown, Right)]);
        assert_eq!(deaths(&mut w), [(0, OwnBody)]);
    }

    #[test]
    fn open_edges_wrap_and_solid_ones_kill() {
        let mut open = world(rules(WallMode::Open), &[(&[(0, 5), (1, 5)], Left)]);
        assert_eq!(deaths(&mut open), []);
        assert_eq!(body(&open, 0), [(29, 5), (0, 5)]);

        let mut solid = world(rules(WallMode::Full), &[(&[(1, 5), (2, 5)], Left)]);
        assert_eq!(deaths(&mut solid), [(0, Wall)]);
    }

    #[test]
    fn pass_through_self_never_passes_through_others() {
        let ghost = Rules {
            pass_through_self: true,
            ..rules(WallMode::Open)
        };
        // Up from (5, 5) lands mid-body at (5, 4).
        let coil: &[(u16, u16)] = &[(5, 5), (6, 5), (6, 4), (5, 4), (4, 4)];
        let mut w = world(rules(WallMode::Open), &[(coil, Up)]);
        assert_eq!(deaths(&mut w), [(0, OwnBody)]);
        let mut w = world(ghost.clone(), &[(coil, Up)]);
        assert_eq!(deaths(&mut w), []);

        let mut w = world(
            ghost,
            &[(&[(5, 5), (4, 5)], Right), (&[(6, 4), (6, 5), (6, 6)], Up)],
        );
        assert_eq!(deaths(&mut w), [(0, OtherSnake)]);
    }

    #[test]
    fn poison_is_eaten_and_kills() {
        let mut w = world(rules(WallMode::Open), &[(&[(5, 5), (4, 5)], Right)]);
        w.place_food(Point { x: 6, y: 5 }, FoodKind::Poison);
        assert_eq!(deaths(&mut w), [(0, Poison)]);
        assert_eq!(body(&w, 0), [(6, 5), (5, 5)], "dies on the poison");
        assert!(w.foods.is_empty());
    }

    #[test]
    fn two_heads_on_one_food_both_die_and_leave_it() {
        let mut w = world(
            rules(WallMode::Open),
            &[(&[(5, 5), (4, 5)], Right), (&[(7, 5), (8, 5)], Left)],
        );
        w.place_food(Point { x: 6, y: 5 }, FoodKind::Normal);
        assert_eq!(deaths(&mut w), [(0, OtherSnake), (1, OtherSnake)]);
        assert_eq!(w.foods.len(), 1);
    }

    #[test]
    fn food_grows_the_snake_that_eats_it() {
        let mut w = world(rules(WallMode::Open), &[(&[(5, 5), (4, 5)], Right)]);
        w.place_food(Point { x: 6, y: 5 }, FoodKind::Super);
        let outcome = w.step();
        assert_eq!(outcome.eaten, [(0, FoodKind::Super)]);
        assert_eq!(w.snakes[0].len(), 2 + 3);
    }

    #[test]
    fn dead_snakes_turn_into_food_when_asked() {
        let mut w = world(
            rules(WallMode::Open),
            &[
                (&[(5, 5), (4, 5), (3, 5)], Right),
                (&[(6, 4), (6, 5), (6, 6)], Up),
            ],
        );
        w.corpses_to_food = true;
        assert_eq!(deaths(&mut w), [(0, OtherSnake)]);
        assert!(w.snakes[0].body.is_empty());
        let mut food: Vec<(u16, u16)> = w.foods.iter().map(|f| (f.pos.x, f.pos.y)).collect();
        food.sort();
        assert_eq!(food, [(3, 5), (4, 5), (5, 5)]);
    }

    #[test]
    fn a_snake_whose_player_left_turns_into_food() {
        let mut w = world(
            rules(WallMode::Open),
            &[(&[(5, 5), (4, 5)], Right), (&[(5, 8), (4, 8)], Right)],
        );
        w.corpses_to_food = true;
        assert!(w.remove(1));
        assert!(!w.remove(1), "already gone");
        assert_eq!(w.snakes[1].death, Some(DeathCause::Left));
        assert_eq!(w.alive(), 1);
        assert_eq!(w.foods.len(), 2);
        assert_eq!(deaths(&mut w), [], "a removed snake doesn't move");
    }

    #[test]
    fn food_comes_faster_with_more_players() {
        let rules = Rules {
            food_ttl_s: 0,
            super_interval_s: 0,
            poison_interval_s: 0,
            ..Rules::default()
        };
        let mut many = World::with_players(rules.clone(), 1, 6).unwrap();
        many.advance_clock(Duration::from_secs(1));
        assert_eq!(many.foods.len(), 3, "one per two snakes");
        many.remove(0);
        many.remove(1);
        many.remove(2);
        let before = many.foods.len();
        many.advance_clock(Duration::from_secs(3));
        assert_eq!(many.foods.len() - before, 2, "three left: two items");

        let mut solo = World::solo(rules, 1).unwrap();
        solo.advance_clock(Duration::from_secs(1));
        assert_eq!(solo.foods.len(), 1);
    }

    /// Runs a multiplayer world with scripted turns: `(tick, snake, dir)`.
    fn run_script(seed: u64, script: &[(usize, usize, Direction)]) -> MatchView {
        let rules = Rules {
            width: 24,
            height: 16,
            inner_walls: InnerWalls::Few,
            food_interval_s: 1,
            ..Rules::default()
        };
        let tick = Duration::from_millis(rules.tick_ms);
        let mut world = World::with_players(rules, seed, 4).unwrap();
        for t in 0..120 {
            for &(_, who, dir) in script.iter().filter(|s| s.0 == t) {
                world.steer(who as u8, dir);
            }
            world.advance_clock(tick);
            world.step();
        }
        world.view()
    }

    #[test]
    fn same_seed_and_inputs_replay_exactly() {
        let script = [
            (3, 0, Up),
            (3, 1, Left),
            (10, 2, Down),
            (11, 3, Right),
            (40, 0, Left),
        ];
        assert_eq!(run_script(42, &script), run_script(42, &script));
        assert_ne!(run_script(42, &script), run_script(43, &script));
    }

    /// A walled 30×20 board with guns on, where dead snakes turn into food,
    /// holding just the given snakes.
    fn armed(wall_mode: WallMode, snakes: &[(&[(u16, u16)], Direction)]) -> World {
        let rules = Rules {
            guns: true,
            ..rules(wall_mode)
        };
        let mut world = world(rules, snakes);
        world.corpses_to_food = true;
        world
    }

    /// Snake `id` takes a gun of `kind` and fires it on the next step.
    fn fire(world: &mut World, id: u8, kind: Item) {
        world.snakes[id as usize].held = Some(Held {
            kind,
            until: world.clock + Duration::from_secs(5),
        });
        assert!(world.fire(id));
    }

    fn wall(world: &mut World, x: u16, y: u16) {
        let mut walls = world.walls.to_vec();
        walls[y as usize * world.width as usize + x as usize] = true;
        world.walls = walls.into();
    }

    fn food_cells(world: &World) -> Vec<(u16, u16)> {
        let mut cells: Vec<_> = world.foods.iter().map(|f| (f.pos.x, f.pos.y)).collect();
        cells.sort();
        cells
    }

    fn hit(shooter: u8, target: u8, left: usize) -> Hit {
        Hit {
            shooter,
            target,
            left,
        }
    }

    use DeathCause::Shot;
    use Item::{Flashbang, Grenade, Laser, Pistol, Smoke};

    #[test]
    fn a_laser_cuts_every_snake_in_its_line_up_to_a_wall() {
        let mut w = armed(
            WallMode::Full,
            &[
                (&[(5, 5), (4, 5)], Right),
                // Crosses the line with its third cell, once it has moved.
                (&[(10, 3), (10, 4), (10, 5), (10, 6)], Up),
                // Puts its head in the line.
                (&[(15, 6), (15, 7)], Up),
                // Behind the wall.
                (&[(22, 6), (22, 5), (22, 4)], Down),
            ],
        );
        wall(&mut w, 20, 5);
        fire(&mut w, 0, Laser);
        let out = w.step();
        assert_eq!(out.deaths, [(2, Shot)]);
        assert_eq!(out.hits, [hit(0, 1, 3), hit(0, 2, 0)]);
        assert_eq!(body(&w, 1), [(10, 2), (10, 3), (10, 4)]);
        assert_eq!(
            body(&w, 3),
            [(22, 7), (22, 6), (22, 5)],
            "the wall stops it"
        );
        assert_eq!(
            food_cells(&w),
            [(10, 5), (15, 5), (15, 6)],
            "the cut piece and the dead snake"
        );
        assert_eq!(w.snakes[0].held, None, "it fires once");
        let beam = &w.beams[0].cells;
        assert_eq!(
            (beam[0], beam[beam.len() - 1]),
            (Point { x: 7, y: 5 }, Point { x: 19, y: 5 })
        );
        w.advance_clock(BEAM_SHOWN);
        assert!(w.beams.is_empty(), "it shows only a moment");
    }

    #[test]
    fn a_laser_stops_at_the_edge_where_snakes_would_wrap() {
        let mut w = armed(
            WallMode::Open,
            &[
                (&[(25, 5), (24, 5)], Right),
                (&[(2, 4), (2, 5), (2, 6)], Up),
            ],
        );
        fire(&mut w, 0, Laser);
        assert_eq!(w.step().hits, []);
        assert_eq!(w.beams[0].cells.last(), Some(&Point { x: 29, y: 5 }));
    }

    #[test]
    fn a_snake_hit_twice_keeps_what_comes_before_the_nearest_hit() {
        let mut w = armed(
            WallMode::Full,
            &[
                (&[(5, 5), (4, 5)], Right),
                // Crosses the line twice once it has moved: cells 2 and 6.
                (
                    &[
                        (10, 4),
                        (10, 5),
                        (10, 6),
                        (11, 6),
                        (12, 6),
                        (12, 5),
                        (12, 4),
                        (12, 3),
                    ],
                    Up,
                ),
            ],
        );
        fire(&mut w, 0, Laser);
        assert_eq!(w.step().hits, [hit(0, 1, 2)]);
        assert_eq!(body(&w, 1), [(10, 3), (10, 4)]);
    }

    #[test]
    fn a_snake_never_shoots_itself() {
        let coiled: &[(u16, u16)] = &[(5, 5), (5, 6), (6, 6), (7, 6), (8, 6), (8, 5), (8, 4)];
        let mut w = armed(
            WallMode::Full,
            &[(coiled, Right), (&[(12, 3), (12, 4), (12, 5), (12, 6)], Up)],
        );
        fire(&mut w, 0, Laser);
        assert_eq!(w.step().hits, [hit(0, 1, 3)], "through its own body");
        assert_eq!(w.snakes[0].len(), 7);
    }

    #[test]
    fn snakes_that_shoot_each_other_both_die() {
        // Heads two cells apart once they've moved: the bullet gets there in
        // the same step as the laser.
        let mut w = armed(
            WallMode::Full,
            &[(&[(5, 5), (4, 5)], Right), (&[(9, 5), (10, 5)], Left)],
        );
        fire(&mut w, 0, Laser);
        fire(&mut w, 1, Pistol);
        assert_eq!(w.step().deaths, [(0, Shot), (1, Shot)]);
    }

    #[test]
    fn a_bullet_flies_three_cells_a_step_and_stops_at_the_first_snake() {
        let column = |x: u16| -> Vec<(u16, u16)> { (1..=7).rev().map(|y| (x, y)).collect() };
        let (first, second) = (column(13), column(16));
        let mut w = armed(
            WallMode::Full,
            &[(&[(5, 5), (4, 5)], Right), (&first, Down), (&second, Down)],
        );
        fire(&mut w, 0, Pistol);
        let bullet = |x| Bullet {
            owner: 0,
            pos: Point { x, y: 5 },
            dir: Right,
        };
        assert_eq!(w.step().hits, []);
        assert_eq!(w.bullets, [bullet(9)]);
        assert_eq!(w.step().hits, []);
        assert_eq!(w.bullets, [bullet(12)], "faster than the snake behind it");
        assert_eq!(w.step().hits, [hit(0, 1, 5)]);
        assert!(w.bullets.is_empty(), "it stops at the first");
        assert_eq!(w.snakes[2].len(), 7);
    }

    #[test]
    fn a_bullet_stops_at_walls_and_edges() {
        for (wall_mode, from) in [(WallMode::Full, 23), (WallMode::Open, 24)] {
            let mut w = armed(wall_mode, &[(&[(from, 5), (from - 1, 5)], Right)]);
            fire(&mut w, 0, Pistol);
            w.step();
            assert_eq!(w.bullets.len(), 1, "{wall_mode:?}");
            w.step();
            assert!(w.bullets.is_empty(), "{wall_mode:?}: it never wraps");
        }
    }

    #[test]
    fn a_head_coming_at_a_bullet_is_hit() {
        let mut w = armed(
            WallMode::Full,
            &[(&[(5, 15), (4, 15)], Right), (&[(9, 5), (8, 5)], Right)],
        );
        w.bullets.push(Bullet {
            owner: 0,
            pos: Point { x: 10, y: 5 },
            dir: Left,
        });
        assert_eq!(w.step().deaths, [(1, Shot)], "it moved onto the bullet");
    }

    #[test]
    fn a_gun_taken_replaces_the_one_held() {
        let mut w = armed(WallMode::Full, &[(&[(5, 5), (4, 5)], Right)]);
        assert!(!w.fire(0), "nothing to fire");
        w.snakes[0].held = Some(Held {
            kind: Laser,
            until: Duration::from_secs(1),
        });
        w.items.push(Pickup {
            pos: Point { x: 6, y: 5 },
            kind: Pistol,
            expires_at: Duration::from_secs(60),
        });
        w.step();
        assert_eq!(
            w.snakes[0].held,
            Some(Held {
                kind: Pistol,
                until: w.clock + Duration::from_secs(w.rules.gun_hold_s),
            })
        );
        assert!(w.items.is_empty());
    }

    #[test]
    fn guns_come_and_go_on_their_clocks() {
        let rules = Rules {
            guns: true,
            gun_interval_s: 10,
            gun_ttl_s: 8,
            gun_hold_s: 5,
            thrown_interval_s: 0,
            ..Rules::default()
        };
        let mut w = World::with_players(rules.clone(), 3, 2).unwrap();
        let second = Duration::from_secs(1);
        let mut on_board = Vec::new();
        for _ in 0..30 {
            w.advance_clock(second);
            on_board.push(w.items.len());
        }
        // One appears at 10 s and 20 s and 30 s, each gone 8 s later.
        let expected: Vec<usize> = (1..=30)
            .map(|t| usize::from(t >= 10 && t % 10 < 8))
            .collect();
        assert_eq!(on_board, expected);

        w.snakes[0].held = Some(Held {
            kind: Laser,
            until: w.clock + Duration::from_secs(5),
        });
        w.advance_clock(Duration::from_secs(4));
        assert!(w.snakes[0].held.is_some());
        w.advance_clock(second);
        assert_eq!(w.snakes[0].held, None, "too late to fire it");

        let mut solo = World::solo(rules, 3).unwrap();
        solo.advance_clock(Duration::from_secs(60));
        assert!(solo.items.is_empty(), "single player has none");
    }

    #[test]
    fn thrown_items_come_on_their_own_clock() {
        let rules = Rules {
            guns: true,
            gun_interval_s: 60,
            thrown_interval_s: 5,
            thrown_ttl_s: 3,
            ..Rules::default()
        };
        let mut w = World::with_players(rules, 3, 2).unwrap();
        w.advance_clock(Duration::from_secs(5));
        assert_eq!(w.items.len(), 1);
        assert!(w.items[0].kind.thrown(), "{:?}", w.items[0].kind);
        w.advance_clock(Duration::from_secs(3));
        assert!(w.items.is_empty(), "gone after its lifetime");
    }

    /// Snake `id` takes a thrown item of `kind` and starts throwing it.
    fn throw(world: &mut World, id: u8, kind: Item) {
        world.snakes[id as usize].held = Some(Held {
            kind,
            until: world.clock + Duration::from_secs(5),
        });
        assert!(world.fire(id), "it starts");
        assert!(world.snakes[id as usize].throw.is_some());
        assert_eq!(
            world.snakes[id as usize].held, None,
            "in the hand, not held"
        );
    }

    #[test]
    fn a_throws_mark_crosses_the_board_and_it_lands_where_let_go() {
        let mut w = armed(WallMode::Full, &[(&[(5, 5), (4, 5)], Right)]);
        throw(&mut w, 0, Smoke);
        w.step();
        let mark = |w: &World| w.snakes[0].throw.as_ref().and_then(|t| t.mark);
        assert_eq!(mark(&w), Some(Point { x: 6, y: 5 }), "from the head");
        w.step();
        assert_eq!(mark(&w), Some(Point { x: 8, y: 5 }), "two cells a step");
        assert_eq!(
            w.view().marks,
            [(Point { x: 8, y: 5 }, Smoke)],
            "everyone sees it"
        );
        assert!(w.release(0));
        w.step();
        assert!(w.snakes[0].throw.is_none());
        assert!(w.smoky(Point { x: 8, y: 5 }) && w.smoky(Point { x: 11, y: 5 }));
        assert!(!w.smoky(Point { x: 12, y: 5 }), "a radius of 3");
        w.advance_clock(Duration::from_secs(w.rules.smoke_s));
        assert!(w.smoke.is_empty(), "it clears");
    }

    #[test]
    fn a_second_f_throws_as_letting_go_does() {
        let mut w = armed(WallMode::Full, &[(&[(5, 5), (4, 5)], Right)]);
        throw(&mut w, 0, Smoke);
        w.step();
        assert!(w.fire(0), "f again");
        w.step();
        assert_eq!(w.smoke.len(), 1);
    }

    #[test]
    fn a_mark_that_passes_the_edge_loses_the_throw() {
        let mut w = armed(WallMode::Open, &[(&[(20, 5), (19, 5)], Right)]);
        throw(&mut w, 0, Grenade);
        for _ in 0..8 {
            w.step();
        }
        assert!(w.snakes[0].throw.is_none(), "gone past the edge");
        assert!(
            w.bursts.is_empty() && !w.release(0),
            "and it never went off"
        );
    }

    #[test]
    fn a_grenade_blasts_everyone_near_but_its_thrower() {
        let mut w = armed(
            WallMode::Full,
            &[
                (&[(5, 5), (4, 5)], Right),
                // Its head lands within the blast once it has moved.
                (&[(6, 7), (7, 7), (8, 7)], Left),
                // Only its second cell does.
                (&[(8, 4), (8, 5), (8, 6), (8, 7)], Up),
            ],
        );
        // Let go at once: it lands at the thrower's own head.
        throw(&mut w, 0, Grenade);
        assert!(w.release(0));
        let out = w.step();
        assert_eq!(out.hits, [hit(0, 1, 0), hit(0, 2, 1)]);
        assert_eq!(out.deaths, [(1, Shot)]);
        assert_eq!(w.snakes[0].len(), 2, "never the thrower");
        assert_eq!(w.view().bursts.len(), 1, "the blast shows a moment");
    }

    #[test]
    fn a_flashbang_blinds_everyone_but_its_thrower() {
        let mut w = armed(
            WallMode::Full,
            &[
                (&[(5, 5), (4, 5)], Right),
                (&[(5, 15), (4, 15)], Right),
                (&[(20, 10), (21, 10)], Left),
            ],
        );
        throw(&mut w, 0, Flashbang);
        w.release(0);
        w.step();
        let blind: Vec<bool> = w.snakes.iter().map(|s| s.blind(w.clock)).collect();
        assert_eq!(blind, [false, true, true]);
        assert_eq!(
            w.view().snakes[1].blind,
            Some(Duration::from_secs(w.rules.flash_s))
        );
        w.advance_clock(Duration::from_secs(w.rules.flash_s));
        assert!(!w.snakes[1].blind(w.clock), "for a while");
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(64))]

        #[test]
        fn living_snakes_never_overlap(
            seed in any::<u64>(),
            players in 2usize..=4,
            wrap in any::<bool>(),
            // A key's second half is a turn, a gun to fire at once, an item
            // to throw, or letting go of one.
            keys in prop::collection::vec((0usize..4, 0usize..10), 0..150),
        ) {
            let rules = Rules {
                width: 20,
                height: 14,
                wall_mode: if wrap { WallMode::Open } else { WallMode::Full },
                inner_walls: InnerWalls::Off,
                food_interval_s: 1,
                super_interval_s: 2,
                growth_mode: if seed % 2 == 0 { GrowthMode::Instant } else { GrowthMode::AtTail },
                guns: true,
                gun_interval_s: 3,
                ..Rules::default()
            };
            let (w, h) = (rules.width, rules.height);
            let world = World::with_players(rules, seed, players);
            prop_assume!(world.is_ok());
            let mut world = world.unwrap();
            let dirs = [Up, Down, Left, Right];
            for (who, key) in keys {
                let until = world.clock + Duration::from_secs(5);
                if let Some(s) = world.snakes.get_mut(who % players) && s.alive() {
                    match key {
                        0..4 => {
                            s.enqueue(dirs[key], w, h);
                        }
                        4..6 => {
                            let kind = Item::GUNS[key - 4];
                            s.held = Some(Held { kind, until });
                            s.firing = true;
                        }
                        6..9 => {
                            s.held = Some(Held { kind: Item::THROWN[key - 6], until });
                        }
                        _ => {}
                    }
                    match key {
                        6..9 => {
                            world.fire((who % players) as u8);
                        }
                        9 => {
                            world.release((who % players) as u8);
                        }
                        _ => {}
                    }
                }
                world.advance_clock(Duration::from_millis(150));
                world.step();

                // Each living snake's cells are distinct, apart from the doubled
                // tail instant growth leaves, and no two snakes share a cell.
                let mut owner: HashMap<Point, u8> = HashMap::new();
                for s in world.snakes.iter().filter(|s| s.alive()) {
                    prop_assert!(!s.body.is_empty(), "snake {} has no head", s.id);
                    let cells: Vec<Point> = s.body.iter().copied().collect();
                    let first_repeat = (1..cells.len())
                        .find(|&i| cells[i] == cells[i - 1])
                        .unwrap_or(cells.len());
                    let (distinct, doubled) = cells.split_at(first_repeat);
                    prop_assert!(
                        doubled.iter().all(|&p| p == cells[first_repeat - 1]),
                        "snake {} repeats a cell mid-body", s.id
                    );
                    let mut sorted = distinct.to_vec();
                    sorted.sort_by_key(|p| (p.y, p.x));
                    sorted.dedup();
                    prop_assert_eq!(sorted.len(), distinct.len(), "snake {} overlaps itself", s.id);
                    for &p in distinct {
                        let prev = owner.insert(p, s.id);
                        prop_assert!(prev.is_none(), "snakes {:?} and {} share {:?}", prev, s.id, p);
                    }
                }
            }
        }
    }
}

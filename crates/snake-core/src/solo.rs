//! Single player: one snake, instant or on-tick moves, a speed boost from
//! super food, and a win for filling the whole board. Or against bots, by
//! rooms' rules: every snake on the tick, no boost, and guns if they're on.
//! The game ends when your snake does, or in a win once every bot is out.

use std::collections::HashSet;
use std::time::Duration;

use anyhow::Result;
use rand::RngExt;

use crate::bot;
use crate::config::{MoveTiming, Rules};
use crate::geom::Direction;
use crate::snake::{DeathCause, Snake};
use crate::world::{FoodKind, World};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GameOverReason {
    HitWall,
    HitSelf,
    /// Into a bot, or head on with one.
    HitBot,
    Shot,
    Poisoned,
    MaxLength,
    /// Every bot is out, and the player's snake isn't.
    BeatBots,
}

pub struct SoloGame {
    pub world: World,
    /// The game waits, frozen, for the first direction key.
    pub started: bool,
    pub over: Option<GameOverReason>,
    tick_accum: Duration,
    boost_until: Duration,
}

impl SoloGame {
    pub fn new(rules: Rules) -> Result<Self> {
        Self::with_seed(rules, rand::rng().random())
    }

    /// A game that plays out the same way every time for the same keys.
    /// Fails if the board has no room for the snake, and its bots.
    pub fn with_seed(rules: Rules, seed: u64) -> Result<Self> {
        let rules = rules.for_solo();
        let world = match rules.bots {
            0 => World::solo(rules, seed)?,
            bots => {
                let mut world = World::with_players(rules, seed, 1 + bots as usize)?;
                // Dead bots turn into food, but the player's snake stays to
                // be seen, and measured, at the end.
                world.keeps_body = Some(0);
                world
            }
        };
        Ok(Self {
            world,
            started: false,
            over: None,
            tick_accum: Duration::ZERO,
            boost_until: Duration::ZERO,
        })
    }

    pub fn rules(&self) -> &Rules {
        &self.world.rules
    }

    pub fn snake(&self) -> &Snake {
        &self.world.snakes[0]
    }

    /// The bots still playing.
    pub fn bots_left(&self) -> usize {
        self.world.snakes[1..].iter().filter(|s| s.alive()).count()
    }

    /// `f`: fires the gun the snake holds, on the next tick, or starts
    /// throwing a thrown item, or, already throwing, throws it.
    pub fn fire(&mut self) {
        if self.started && self.over.is_none() {
            self.world.fire(0);
        }
    }

    /// `f` let go: a throw on its way lands, on the next tick.
    pub fn release(&mut self) {
        if self.started && self.over.is_none() {
            self.world.release(0);
        }
    }

    /// The snake's length.
    #[expect(clippy::len_without_is_empty, reason = "a snake is never empty")]
    pub fn len(&self) -> usize {
        self.snake().len()
    }

    pub fn clock(&self) -> Duration {
        self.world.clock
    }

    pub fn boost_active(&self) -> bool {
        self.world.clock < self.boost_until
    }

    pub fn tick_interval(&self) -> Duration {
        let base = Duration::from_millis(self.world.rules.tick_ms);
        if self.boost_active() { base / 2 } else { base }
    }

    /// How long until the next automatic step, while the game is running.
    /// A runner can sleep until then instead of polling.
    pub fn until_next_step(&self) -> Option<Duration> {
        (self.started && self.over.is_none())
            .then(|| self.tick_interval().saturating_sub(self.tick_accum))
    }

    /// A direction key; the first one starts the game. With instant moves the
    /// snake steps right away, otherwise the turn waits for the next tick.
    pub fn turn(&mut self, dir: Direction) {
        if self.over.is_some() {
            return;
        }
        let (w, h) = (self.world.width, self.world.height);
        match self.world.rules.move_timing {
            MoveTiming::Instant => {
                if !self.world.snakes[0].set_dir(dir, w, h) {
                    return;
                }
                self.started = true;
                self.tick_accum = Duration::ZERO;
                self.step();
            }
            MoveTiming::OnTick => {
                if self.world.snakes[0].enqueue(dir, w, h) {
                    self.started = true;
                }
            }
        }
    }

    pub fn advance(&mut self, dt: Duration) {
        if self.over.is_some() || !self.started {
            return;
        }
        let dt = dt.min(Duration::from_millis(250));
        self.world.advance_clock(dt);
        self.tick_accum += dt;
        while self.over.is_none() && self.tick_accum >= self.tick_interval() {
            self.tick_accum -= self.tick_interval();
            self.step();
        }
    }

    fn step(&mut self) {
        for id in 1..self.world.snakes.len() as u8 {
            bot::drive(&mut self.world, id);
        }
        let outcome = self.world.step();
        if let Some(&(_, cause)) = outcome.deaths.iter().find(|(id, _)| *id == 0) {
            self.over = Some(match cause {
                DeathCause::Wall => GameOverReason::HitWall,
                // The player never leaves mid-step.
                DeathCause::OwnBody | DeathCause::Left => GameOverReason::HitSelf,
                DeathCause::OtherSnake => GameOverReason::HitBot,
                DeathCause::Shot => GameOverReason::Shot,
                DeathCause::Poison => GameOverReason::Poisoned,
            });
            return;
        }
        if self.world.rules.bots > 0 && self.bots_left() == 0 {
            self.over = Some(GameOverReason::BeatBots);
            return;
        }
        let boost = self.world.rules.boost_duration_s;
        if boost > 0 && outcome.eaten.iter().any(|&(_, k)| k == FoodKind::Super) {
            self.boost_until = self.world.clock + Duration::from_secs(boost);
        }
        let free = self.world.free_cells();
        let body = &self.snake().body;
        if body.len() >= free && body.iter().collect::<HashSet<_>>().len() >= free {
            self.over = Some(GameOverReason::MaxLength);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;

    use super::*;
    use crate::config::{InnerWalls, WallMode};
    use crate::geom::Point;

    fn game(timing: MoveTiming) -> SoloGame {
        let rules = Rules {
            wall_mode: WallMode::Open,
            inner_walls: InnerWalls::Off,
            move_timing: timing,
            ..Rules::default()
        };
        let mut game = SoloGame::with_seed(rules, 1).unwrap();
        let body = VecDeque::from([Point { x: 5, y: 5 }, Point { x: 4, y: 5 }]);
        game.world.snakes[0] = Snake::new(0, body, Direction::Right);
        game
    }

    fn head(game: &SoloGame) -> (u16, u16) {
        let p = game.snake().head();
        (p.x, p.y)
    }

    #[test]
    fn snake_wraps_on_open_board() {
        let mut game = game(MoveTiming::Instant);
        game.world.snakes[0] =
            Snake::new(0, VecDeque::from([Point { x: 0, y: 5 }]), Direction::Left);
        game.turn(Direction::Left);
        assert_eq!(head(&game), (game.world.width - 1, 5));
        assert!(game.over.is_none());
    }

    #[test]
    fn instant_moves_right_away() {
        let mut game = game(MoveTiming::Instant);
        game.turn(Direction::Up);
        assert!(game.started);
        assert_eq!(head(&game), (5, 4));
    }

    #[test]
    fn on_tick_moves_wait_for_the_tick_and_queue_up() {
        let mut game = game(MoveTiming::OnTick);
        let tick = game.tick_interval();
        game.turn(Direction::Up);
        game.turn(Direction::Left);
        assert!(game.started);
        assert_eq!(head(&game), (5, 5), "nothing moves before the tick");
        game.advance(tick);
        assert_eq!(head(&game), (5, 4));
        game.advance(tick);
        assert_eq!(head(&game), (4, 4), "the second key waited its turn");
    }

    #[test]
    fn a_reversal_neither_moves_nor_starts() {
        for timing in [MoveTiming::Instant, MoveTiming::OnTick] {
            let mut game = game(timing);
            game.turn(Direction::Left);
            assert!(!game.started, "{timing:?}");
            assert_eq!(head(&game), (5, 5));
        }
    }

    #[test]
    fn outlasting_every_bot_wins() {
        let rules = Rules {
            bots: 2,
            ..Rules::default()
        };
        let mut game = SoloGame::with_seed(rules, 1).unwrap();
        game.started = true;
        let tick = game.tick_interval();
        game.world.snakes[1].death = Some(DeathCause::Wall);
        game.advance(tick);
        assert_eq!(game.over, None, "one bot is left");
        game.world.snakes[2].death = Some(DeathCause::Wall);
        game.advance(tick);
        assert_eq!(game.over, Some(GameOverReason::BeatBots));
        assert!(game.snake().alive());
    }

    #[test]
    fn same_seed_and_keys_replay_exactly() {
        let play = |seed| {
            let rules = Rules {
                inner_walls: InnerWalls::Medium,
                move_timing: MoveTiming::OnTick,
                food_interval_s: 1,
                ..Rules::default()
            };
            let mut game = SoloGame::with_seed(rules, seed).unwrap();
            let dirs = [
                Direction::Up,
                Direction::Left,
                Direction::Down,
                Direction::Right,
            ];
            for i in 0..200 {
                game.turn(dirs[i / 7 % 4]);
                game.advance(Duration::from_millis(40));
            }
            (game.world.view(), game.over)
        };
        assert_eq!(play(9), play(9));
    }

    /// A walled board against `bots`, with the player's snake put at (5, 5)
    /// heading right and every bot's far off in the bottom row.
    fn against(bots: u8, guns: bool) -> SoloGame {
        let rules = Rules {
            bots,
            guns,
            food_interval_s: 60,
            super_interval_s: 0,
            poison_interval_s: 0,
            ..Rules::default()
        };
        let mut game = SoloGame::with_seed(rules, 2).unwrap();
        let body = VecDeque::from([Point { x: 5, y: 5 }, Point { x: 4, y: 5 }]);
        game.world.snakes[0] = Snake::new(0, body, Direction::Right);
        for id in 1..=bots {
            let x = 4 * u16::from(id) + 6;
            let body = VecDeque::from([Point { x, y: 17 }, Point { x: x - 1, y: 17 }]);
            game.world.snakes[id as usize] = Snake::new(id, body, Direction::Right);
        }
        game.world.foods.clear();
        game
    }

    #[test]
    fn bots_play_by_rooms_rules_and_wait_for_the_first_key() {
        let mut game = against(2, false);
        assert_eq!(game.world.snakes.len(), 3);
        assert_eq!(game.rules().move_timing, MoveTiming::OnTick);
        assert_eq!(game.rules().boost_duration_s, 0);
        let bot = game.world.snakes[1].head();
        game.advance(Duration::from_secs(1));
        assert_eq!(game.world.snakes[1].head(), bot, "frozen until a key");
        game.turn(Direction::Up);
        game.advance(game.tick_interval());
        assert_ne!(game.world.snakes[1].head(), bot, "they move with you");
        assert_eq!(game.bots_left(), 2);
        assert_eq!(head(&game), (5, 4), "on the tick");
    }

    #[test]
    fn guns_come_with_a_bot_and_fire_with_f() {
        let mut game = against(1, true);
        assert_eq!(game.rules().bots, 1, "someone to shoot");
        game.world.snakes[0].held = Some(crate::guns::Held {
            kind: crate::guns::Item::Pistol,
            until: Duration::from_secs(60),
        });
        game.fire();
        assert!(game.world.bullets.is_empty(), "not before the game starts");
        game.turn(Direction::Right);
        game.fire();
        game.advance(game.tick_interval());
        assert_eq!(game.world.bullets.len(), 1);
        assert_eq!(game.snake().held, None);
    }

    #[test]
    fn a_bots_shot_ends_the_game() {
        let mut game = against(1, true);
        // A bullet of the bot's, coming at the player's head.
        game.world.bullets.push(crate::guns::Bullet {
            owner: 1,
            pos: Point { x: 7, y: 5 },
            dir: Direction::Left,
        });
        game.turn(Direction::Right);
        game.advance(game.tick_interval());
        assert_eq!(game.over, Some(GameOverReason::Shot));
        assert_eq!(game.len(), 2, "it stays, to be measured");
    }

    #[test]
    fn too_many_bots_for_the_board_dont_start() {
        let rules = Rules {
            width: 10,
            height: 8,
            bots: 7,
            ..Rules::default()
        };
        assert!(SoloGame::with_seed(rules, 1).is_err());
    }
}

//! A computer player. Each step it heads for the nearest food it can reach,
//! never into a wall, a body or poison while it has another way, and away
//! from moves that would box it in or meet another head. Where there are
//! items, it goes for them as for food while it has none; it fires when
//! another snake is in its line of fire, and throws as players do, letting
//! go once its mark gets where it aims. It can't see into smoke. Blinded
//! by a flashbang, it follows a few moves it planned on the board as it
//! last saw it, then goes straight.

use std::collections::{HashSet, VecDeque};

use crate::geom::{Direction, Point, wrap_next};
use crate::guns::{Item, disc, next_cell};
use crate::world::{FoodKind, World};

/// Further than this, a pistol's bullet would likely miss a moving snake,
/// so bots hold their fire.
const PISTOL_RANGE: usize = 12;

const DIRECTIONS: [Direction; 4] = [
    Direction::Up,
    Direction::Down,
    Direction::Left,
    Direction::Right,
];

/// Snake `id`'s turn as a bot on `world`: which way it goes, and whether it
/// fires, starts a throw or lets one go.
pub fn drive(world: &mut World, id: u8) {
    let Some(i) = world.snakes.iter().position(|s| s.id == id && s.alive()) else {
        return;
    };
    if world.snakes[i].blind(world.clock) {
        if world.snakes[i].plan.is_none() {
            let k = world.roll(2..=8);
            world.snakes[i].plan = Some(remember(world, id, k));
        }
        if let Some(dir) = world.snakes[i].plan.as_mut().and_then(VecDeque::pop_front) {
            world.steer(id, dir);
        }
        return;
    }
    world.snakes[i].plan = None;
    let Some(dir) = choose(world, id) else {
        return;
    };
    world.steer(id, dir);
    let snake = &world.snakes[i];
    if let Some(throw) = &snake.throw {
        let there = throw.mark.is_some() && snake.aim_for.is_none_or(|d| throw.travelled >= d);
        if !throw.released && there {
            world.release(id);
        }
        return;
    }
    match snake.held.map(|h| h.kind) {
        Some(kind) if kind.thrown() => {
            if let Some(d) = aim(world, id, dir, kind) {
                world.snakes[i].aim_for = Some(d);
                world.fire(id);
            }
        }
        Some(_) if fires(world, id, dir) => {
            world.fire(id);
        }
        _ => {}
    }
}

/// The next `k` moves snake `id` would make, planned on a copy of the
/// board as it is: fewer if it would die before.
fn remember(world: &World, id: u8, k: usize) -> VecDeque<Direction> {
    let mut sim = world.clone();
    let mut plan = VecDeque::new();
    for _ in 0..k {
        let Some(dir) = choose(&sim, id) else {
            break;
        };
        plan.push_back(dir);
        sim.steer(id, dir);
        sim.step();
    }
    plan
}

/// How far snake `id`, about to go `dir`, would send a throw's mark before
/// letting go of `kind`; `None` to keep it for now. A grenade waits for a
/// cell whose blast would reach another snake; a flashbang goes at once;
/// smoke goes toward the nearest other snake in its line, or a little ahead.
fn aim(world: &World, id: u8, dir: Direction, kind: Item) -> Option<usize> {
    let me = world.snakes.iter().find(|s| s.id == id && s.alive())?;
    let (w, h) = (world.width, world.height);
    // The mark sets off from where the head will be, the way it goes now.
    let mut p = wrap_next(me.head(), dir, w, h);
    let mut line = Vec::new();
    while let Some(next) = next_cell(p, me.dir, w, h) {
        line.push(next);
        p = next;
    }
    let others: HashSet<Point> = world
        .snakes
        .iter()
        .filter(|s| s.alive() && s.id != id)
        .flat_map(|s| s.body.iter().copied())
        .filter(|&p| !world.smoky(p))
        .collect();
    match kind {
        Item::Grenade => {
            let r = world.rules.grenade_radius;
            line.iter()
                .position(|&c| disc(c, r, w, h).iter().any(|p| others.contains(p)))
                .map(|i| (i + 1).max(2))
        }
        Item::Flashbang => (line.len() >= 3).then_some(2),
        Item::Smoke => {
            let to = line
                .iter()
                .position(|p| others.contains(p))
                .map_or(4, |i| i + 1);
            (to < line.len()).then_some(to)
        }
        Item::Laser | Item::Pistol => None,
    }
}

/// Which way snake `id` should go next on `world`; `None` if it's dead.
/// Under smoke it sees only walls and its own body.
pub fn choose(world: &World, id: u8) -> Option<Direction> {
    let me = world.snakes.iter().find(|s| s.id == id && s.alive())?;
    let (w, h) = (world.width, world.height);
    let at = |p: Point| p.y as usize * w as usize + p.x as usize;
    let seen = |p: Point| !world.smoky(p);

    // Cells that kill whoever moves in next step: walls, bodies and poison.
    // A tail moves away, unless the snake is growing and its last cell
    // repeats.
    let mut blocked = vec![false; w as usize * h as usize];
    for y in 0..h {
        for x in 0..w {
            blocked[at(Point { x, y })] = world.wall_at(x, y);
        }
    }
    for snake in world.snakes.iter().filter(|s| s.alive()) {
        if snake.id == id && world.rules.pass_through_self {
            continue;
        }
        let mut body: Vec<Point> = snake.body.iter().copied().collect();
        if body.len() > 1 && body[body.len() - 1] != body[body.len() - 2] {
            body.pop();
        }
        for p in body.into_iter().filter(|&p| snake.id == id || seen(p)) {
            blocked[at(p)] = true;
        }
    }
    for food in world.foods.iter().filter(|f| f.kind == FoodKind::Poison) {
        if seen(food.pos) {
            blocked[at(food.pos)] = true;
        }
    }
    // Where other heads may land next step: a head-on crash kills both.
    let mut contested = vec![false; blocked.len()];
    for snake in world.snakes.iter().filter(|s| s.alive() && s.id != id) {
        if !seen(snake.head()) {
            continue;
        }
        for dir in DIRECTIONS {
            contested[at(wrap_next(snake.head(), dir, w, h))] = true;
        }
    }
    let mut food = vec![false; blocked.len()];
    for f in world
        .foods
        .iter()
        .filter(|f| f.kind != FoodKind::Poison && seen(f.pos))
    {
        food[at(f.pos)] = true;
    }
    if me.held.is_none() && me.throw.is_none() {
        for item in world.items.iter().filter(|g| seen(g.pos)) {
            food[at(item.pos)] = true;
        }
    }

    DIRECTIONS
        .into_iter()
        .filter(|&dir| !me.reverses(dir, w, h))
        .max_by_key(|&dir| {
            let next = wrap_next(me.head(), dir, w, h);
            let safe = !blocked[at(next)];
            let (room, to_food) = if safe {
                explore(next, w, h, &blocked, &food)
            } else {
                (0, None)
            };
            // Most important first.
            (
                safe,
                room >= me.len(),
                !contested[at(next)],
                std::cmp::Reverse(to_food.unwrap_or(usize::MAX)),
                room,
                dir == me.dir,
            )
        })
}

/// Whether snake `id`, about to go `dir`, should fire the gun it holds: it
/// fires once another snake is in its line of fire from where its head
/// will be, anywhere along it for a laser and close for a pistol.
pub fn fires(world: &World, id: u8, dir: Direction) -> bool {
    let Some(me) = world.snakes.iter().find(|s| s.id == id && s.alive()) else {
        return false;
    };
    let Some(gun) = me.held else {
        return false;
    };
    let reach = match gun.kind {
        Item::Laser => usize::MAX,
        Item::Pistol => PISTOL_RANGE,
        Item::Grenade | Item::Flashbang | Item::Smoke => return false,
    };
    let others: HashSet<Point> = world
        .snakes
        .iter()
        .filter(|s| s.alive() && s.id != id)
        .flat_map(|s| s.body.iter().copied())
        .filter(|&p| !world.smoky(p))
        .collect();
    let head = wrap_next(me.head(), dir, world.width, world.height);
    world
        .line_of_fire(head, dir, reach)
        .iter()
        .any(|p| others.contains(p))
}

/// From `start`: how many open cells can be reached, and how many steps
/// away the nearest food is.
fn explore(
    start: Point,
    w: u16,
    h: u16,
    blocked: &[bool],
    food: &[bool],
) -> (usize, Option<usize>) {
    let at = |p: Point| p.y as usize * w as usize + p.x as usize;
    let mut seen = vec![false; blocked.len()];
    let mut queue = VecDeque::from([(start, 0)]);
    seen[at(start)] = true;
    let (mut room, mut nearest) = (0, None);
    while let Some((p, steps)) = queue.pop_front() {
        room += 1;
        if nearest.is_none() && food[at(p)] {
            nearest = Some(steps);
        }
        for dir in DIRECTIONS {
            let next = wrap_next(p, dir, w, h);
            if !blocked[at(next)] && !seen[at(next)] {
                seen[at(next)] = true;
                queue.push_back((next, steps + 1));
            }
        }
    }
    (room, nearest)
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;

    use super::*;
    use crate::config::{Rules, WallMode};
    use crate::snake::Snake;
    use crate::world::Food;

    fn p(x: u16, y: u16) -> Point {
        Point { x, y }
    }

    /// A walled 12×10 board with one snake heading right from (5, 5), and
    /// no food unless a test adds some.
    fn board(rules: Rules) -> World {
        let rules = Rules {
            width: 12,
            height: 10,
            food_interval_s: 0,
            super_interval_s: 0,
            poison_interval_s: 0,
            ..rules
        };
        let mut world = World::with_players(rules, 1, 2).unwrap();
        world.snakes.truncate(1);
        world.snakes[0] = Snake::new(
            0,
            VecDeque::from([p(5, 5), p(4, 5), p(3, 5)]),
            Direction::Right,
        );
        world.foods.clear();
        world
    }

    fn food(world: &mut World, pos: Point, kind: FoodKind) {
        world.foods.push(Food {
            pos,
            kind,
            expires_at: None,
        });
    }

    #[test]
    fn it_heads_for_the_nearest_food() {
        let mut world = board(Rules::default());
        food(&mut world, p(5, 2), FoodKind::Normal);
        food(&mut world, p(9, 9), FoodKind::Super);
        assert_eq!(choose(&world, 0), Some(Direction::Up));
        world.foods.clear();
        food(&mut world, p(5, 8), FoodKind::Normal);
        assert_eq!(choose(&world, 0), Some(Direction::Down));
    }

    #[test]
    fn it_never_turns_back_or_into_a_wall_or_poison() {
        let mut world = board(Rules::default());
        // Food behind it can't pull it into its own neck.
        food(&mut world, p(1, 5), FoodKind::Normal);
        assert_ne!(choose(&world, 0), Some(Direction::Left));

        // At the right-hand wall, heading into it.
        world.snakes[0].body = VecDeque::from([p(10, 5), p(9, 5), p(8, 5)]);
        world.foods.clear();
        food(&mut world, p(10, 4), FoodKind::Poison);
        assert_eq!(choose(&world, 0), Some(Direction::Down));
    }

    #[test]
    fn it_wraps_when_the_edges_are_open() {
        let mut world = board(Rules {
            wall_mode: WallMode::Open,
            ..Rules::default()
        });
        world.snakes[0].body = VecDeque::from([p(11, 5), p(10, 5), p(9, 5)]);
        food(&mut world, p(1, 5), FoodKind::Normal);
        assert_eq!(
            choose(&world, 0),
            Some(Direction::Right),
            "through the edge"
        );
    }

    #[test]
    fn it_keeps_out_of_dead_ends() {
        let mut world = board(Rules::default());
        world.snakes[0].body = (1..=5).rev().map(|x| p(x, 5)).collect();
        // Another snake walls in a two-cell pocket above, with food in it.
        // Its tail repeats, so it stays put next step.
        let pocket = [
            (4, 4),
            (4, 3),
            (4, 2),
            (5, 2),
            (6, 2),
            (6, 3),
            (6, 4),
            (6, 4),
        ];
        let other = pocket.into_iter().map(|(x, y)| p(x, y)).collect();
        world.snakes.push(Snake::new(1, other, Direction::Up));
        food(&mut world, p(5, 3), FoodKind::Normal);
        assert_eq!(
            choose(&world, 0),
            Some(Direction::Right),
            "too small for it"
        );
    }

    #[test]
    fn it_has_no_move_once_dead() {
        let mut world = board(Rules::default());
        world.snakes[0].death = Some(crate::snake::DeathCause::Wall);
        assert_eq!(choose(&world, 0), None);
    }

    #[test]
    fn it_fires_when_a_snake_is_in_its_line() {
        use crate::guns::Held;

        let mut world = board(Rules::default());
        let hold = |kind| {
            Some(Held {
                kind,
                until: std::time::Duration::from_secs(5),
            })
        };
        // Another snake across row 5, eight cells ahead.
        let across = (4..=6).map(|y| p(10, y)).collect();
        world.snakes.push(Snake::new(1, across, Direction::Up));
        assert!(!fires(&world, 0, Direction::Right), "no gun");
        world.snakes[0].held = hold(Item::Laser);
        assert!(fires(&world, 0, Direction::Right));
        assert!(!fires(&world, 0, Direction::Up), "nobody that way");
        world.snakes[0].held = hold(Item::Pistol);
        assert!(fires(&world, 0, Direction::Right), "close enough");
    }

    #[test]
    fn it_cant_see_into_smoke() {
        use crate::guns::Cloud;

        let mut world = board(Rules::default());
        food(&mut world, p(5, 2), FoodKind::Normal);
        assert_eq!(choose(&world, 0), Some(Direction::Up));
        world.smoke.push(Cloud {
            cells: vec![p(5, 2)],
            until: std::time::Duration::from_secs(60),
        });
        assert_eq!(
            choose(&world, 0),
            Some(Direction::Right),
            "no food it knows of"
        );
    }

    #[test]
    fn blinded_it_follows_its_plan_then_goes_straight() {
        let mut world = board(Rules::default());
        food(&mut world, p(8, 2), FoodKind::Normal);
        world.snakes[0].blind_until = std::time::Duration::from_secs(60);
        drive(&mut world, 0);
        let left = world.snakes[0].plan.as_ref().map(VecDeque::len).unwrap();
        assert!((1..=7).contains(&left), "planned 2 to 8 moves: {left} left");
        world.step();
        for _ in 0..left {
            drive(&mut world, 0);
            world.step();
        }
        assert_eq!(world.snakes[0].plan.as_ref().map(VecDeque::len), Some(0));
        let dir = world.snakes[0].dir;
        drive(&mut world, 0);
        world.step();
        assert!(
            !world.snakes[0].alive() || world.snakes[0].dir == dir,
            "straight on, out of plan"
        );
    }

    #[test]
    fn it_throws_a_grenade_at_a_snake_ahead() {
        use crate::guns::Held;

        let mut world = board(Rules {
            guns: true,
            ..Rules::default()
        });
        world.corpses_to_food = true;
        let column = (1..=5).rev().map(|y| p(9, y)).collect();
        world.snakes.push(Snake::new(1, column, Direction::Down));
        world.snakes[0].held = Some(Held {
            kind: Item::Grenade,
            until: std::time::Duration::from_secs(60),
        });
        let mut hit = false;
        for _ in 0..6 {
            drive(&mut world, 0);
            hit |= world
                .step()
                .hits
                .iter()
                .any(|h| (h.shooter, h.target) == (0, 1));
        }
        assert!(hit, "the blast reached it");
    }

    #[test]
    fn bots_take_items_and_use_them() {
        let (mut hits, mut thrown) = (0, 0);
        for seed in 0..6 {
            let mut world = World::with_players(crate::round::quick_guns_rules(), seed, 4).unwrap();
            for _ in 0..2000 {
                if world.alive() < 2 {
                    break;
                }
                world.advance_clock(std::time::Duration::from_millis(150));
                for id in 0..4 {
                    drive(&mut world, id);
                }
                let (bursts, smoke) = (world.bursts.len(), world.smoke.len());
                hits += world.step().hits.len();
                thrown += usize::from(world.bursts.len() > bursts || world.smoke.len() > smoke);
            }
        }
        assert!(hits > 0, "no bot ever hit anyone");
        assert!(thrown > 0, "no bot ever threw anything");
    }

    #[test]
    fn four_bots_last_on_a_quick_play_board() {
        for seed in 0..6 {
            let mut world = World::with_players(crate::round::quick_rules(), seed, 4).unwrap();
            let mut steps = 0;
            while world.alive() == 4 && steps < 2000 {
                world.advance_clock(std::time::Duration::from_millis(150));
                for id in 0..4 {
                    if let Some(dir) = choose(&world, id) {
                        world.steer(id, dir);
                    }
                }
                world.step();
                steps += 1;
            }
            assert!(steps >= 50, "seed {seed}: a bot died after {steps} steps");
        }
    }
}

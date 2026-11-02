//! Gun mode's items, in rooms and against bots. Guns and thrown items
//! appear on the board every so often; a snake whose head reaches one has
//! a few seconds to use it, with `f`.
//!
//! - A gun fires straight ahead from the head. A shot that hits a head
//!   kills that snake; one that hits a body cuts the snake there.
//! - A thrown item is held on `f` while a mark crosses the board, and
//!   lands where the mark is when `f` is let go: a grenade's blast hits like
//!   a shot, a flashbang blinds everyone else, and smoke hides what's under
//!   it from everyone.

use std::time::Duration;

use crate::geom::{Direction, Point};

/// How long a laser's beam, or a grenade's blast, stays on the board, to
/// be seen.
pub const BEAM_SHOWN: Duration = Duration::from_millis(300);

/// Cells a throw's mark moves each step.
pub const MARK_SPEED: usize = 2;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum Item {
    /// Hits at once along its whole line, through every snake in it.
    Laser,
    /// One bullet, which stops at the first snake it hits.
    Pistol,
    /// Bursts where it lands, hitting every snake but the thrower's in its
    /// blast.
    Grenade,
    /// Blinds every snake but the thrower's for a while.
    Flashbang,
    /// A cloud that hides what's under it from everyone.
    Smoke,
}

impl Item {
    pub const GUNS: [Item; 2] = [Item::Laser, Item::Pistol];
    pub const THROWN: [Item; 3] = [Item::Grenade, Item::Flashbang, Item::Smoke];

    pub fn name(self) -> &'static str {
        match self {
            Item::Laser => "laser",
            Item::Pistol => "pistol",
            Item::Grenade => "grenade",
            Item::Flashbang => "flashbang",
            Item::Smoke => "smoke",
        }
    }

    /// Whether it's thrown, rather than fired.
    pub fn thrown(self) -> bool {
        Item::THROWN.contains(&self)
    }
}

/// An item on the board, until a snake takes it or it goes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Pickup {
    pub pos: Point,
    pub kind: Item,
    pub expires_at: Duration,
}

/// The item a snake holds, until it uses it or its time runs out.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Held {
    pub kind: Item,
    pub until: Duration,
}

/// A throw on its way: its mark crosses the board until `f` is let go.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Throw {
    pub kind: Item,
    /// The way the snake was going when it started.
    pub dir: Direction,
    /// Where it would land, from the step after it started.
    pub mark: Option<Point>,
    /// Cells the mark has moved.
    pub travelled: usize,
    /// It lands on the next step.
    pub released: bool,
}

/// A pistol's bullet in flight.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Bullet {
    /// The snake that fired it, which it never hits.
    pub owner: u8,
    pub pos: Point,
    pub dir: Direction,
}

/// The cells a laser lit, kept a moment to be seen.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Beam {
    pub dir: Direction,
    pub cells: Vec<Point>,
    pub until: Duration,
}

/// Cells a grenade's blast covered, or a smoke cloud covers, until a time.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Cloud {
    pub cells: Vec<Point>,
    pub until: Duration,
}

/// A shot or a blast that hit a snake during a step.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Hit {
    pub shooter: u8,
    pub target: u8,
    /// How long the target is now: 0 when it killed it.
    pub left: usize,
}

/// The cells of a `w`×`h` board within `radius` of `center`, as a rounded
/// disc: those with dx² + dy² ≤ r(r + 1). It never wraps.
pub fn disc(center: Point, radius: u16, w: u16, h: u16) -> Vec<Point> {
    let r = i32::from(radius);
    let mut cells = Vec::new();
    for dy in -r..=r {
        for dx in -r..=r {
            if dx * dx + dy * dy > r * (r + 1) {
                continue;
            }
            let (x, y) = (i32::from(center.x) + dx, i32::from(center.y) + dy);
            if (0..i32::from(w)).contains(&x) && (0..i32::from(h)).contains(&y) {
                cells.push(Point {
                    x: x as u16,
                    y: y as u16,
                });
            }
        }
    }
    cells
}

/// The cell after `p` going `dir` on a `w`×`h` board, or `None` past its
/// edge: shots never wrap, even where snakes do.
pub fn next_cell(p: Point, dir: Direction, w: u16, h: u16) -> Option<Point> {
    match dir {
        Direction::Up => p.y.checked_sub(1).map(|y| Point { x: p.x, y }),
        Direction::Down => (p.y + 1 < h).then(|| Point { x: p.x, y: p.y + 1 }),
        Direction::Left => p.x.checked_sub(1).map(|x| Point { x, y: p.y }),
        Direction::Right => (p.x + 1 < w).then(|| Point { x: p.x + 1, y: p.y }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shots_stop_at_every_edge() {
        let p = |x, y| Point { x, y };
        assert_eq!(next_cell(p(0, 3), Direction::Left, 10, 8), None);
        assert_eq!(next_cell(p(9, 3), Direction::Right, 10, 8), None);
        assert_eq!(next_cell(p(4, 0), Direction::Up, 10, 8), None);
        assert_eq!(next_cell(p(4, 7), Direction::Down, 10, 8), None);
        assert_eq!(next_cell(p(4, 3), Direction::Right, 10, 8), Some(p(5, 3)));
    }

    #[test]
    fn a_disc_is_round_and_stays_on_the_board() {
        let p = |x, y| Point { x, y };
        assert_eq!(disc(p(5, 5), 1, 10, 10).len(), 9, "a 3×3 square");
        let two = disc(p(5, 5), 2, 10, 10);
        assert_eq!(two.len(), 21, "a 5×5 square without its corners");
        assert!(!two.contains(&p(3, 3)) && two.contains(&p(3, 4)));
        assert_eq!(disc(p(0, 0), 1, 10, 10).len(), 4, "cut by the corner");
    }
}

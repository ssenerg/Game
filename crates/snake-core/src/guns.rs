//! Guns, in rooms that turn them on. One appears on the board every so
//! often; a snake whose head reaches it has a few seconds to fire it,
//! straight ahead from its head. A shot that hits a head kills that snake;
//! one that hits a body cuts the snake there.

use std::time::Duration;

use crate::geom::{Direction, Point};

/// How long a laser's beam stays on the board, to be seen.
pub const BEAM_SHOWN: Duration = Duration::from_millis(300);

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum GunKind {
    /// Hits at once along its whole line, through every snake in it.
    Laser,
    /// One bullet, which stops at the first snake it hits.
    Pistol,
}

impl GunKind {
    pub const ALL: [GunKind; 2] = [GunKind::Laser, GunKind::Pistol];

    pub fn name(self) -> &'static str {
        match self {
            GunKind::Laser => "laser",
            GunKind::Pistol => "pistol",
        }
    }
}

/// A gun on the board, until a snake takes it or it goes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Gun {
    pub pos: Point,
    pub kind: GunKind,
    pub expires_at: Duration,
}

/// The gun a snake holds, until it fires it or its time runs out.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Held {
    pub kind: GunKind,
    pub until: Duration,
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

/// A shot that hit a snake during a step.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Hit {
    pub shooter: u8,
    pub target: u8,
    /// How long the target is now: 0 when the shot killed it.
    pub left: usize,
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
}

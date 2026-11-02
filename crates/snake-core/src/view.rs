use std::sync::Arc;
use std::time::Duration;

use crate::geom::{Direction, Point};
use crate::guns::Item;
use crate::world::FoodKind;

/// Everything needed to draw a board, detached from the [`crate::world::World`]
/// it came from. Cheap to clone: the walls are shared.
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct MatchView {
    pub width: u16,
    pub height: u16,
    /// Row-major, `width * height` cells.
    #[cfg_attr(feature = "serde", serde(with = "packed"))]
    pub walls: Arc<[bool]>,
    pub snakes: Vec<SnakeView>,
    pub foods: Vec<(Point, FoodKind)>,
    /// Items waiting to be taken.
    pub items: Vec<(Point, Item)>,
    /// Pistols' bullets in flight, and which way each goes.
    pub bullets: Vec<(Point, Direction)>,
    /// The cells lasers lit a moment ago, with the way each went.
    pub beams: Vec<(Direction, Vec<Point>)>,
    /// Where throws on their way would land, and what each throws.
    pub marks: Vec<(Point, Item)>,
    /// The cells grenades' blasts and flashbangs' flashes lit a moment ago.
    pub bursts: Vec<(Item, Vec<Point>)>,
    /// Cells under smoke, which hides what's there from everyone.
    pub smoke: Vec<Point>,
    pub clock: Duration,
}

#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct SnakeView {
    pub id: u8,
    /// Head first; empty once a dead snake has turned into food.
    pub body: Vec<Point>,
    pub alive: bool,
    /// The item it holds, and how long it has left to use it.
    pub held: Option<(Item, Duration)>,
    /// An item it's throwing, until it lets go.
    pub throwing: Option<Item>,
    /// How long a flashbang still blinds it.
    pub blind: Option<Duration>,
}

impl MatchView {
    pub fn wall_at(&self, x: u16, y: u16) -> bool {
        self.walls[y as usize * self.width as usize + x as usize]
    }
}

/// Walls on the wire take one bit a cell: a 30×18 board's 540 cells fit in
/// 68 bytes.
#[cfg(feature = "serde")]
mod packed {
    use std::sync::Arc;

    use serde::de::Error;
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    /// More cells than any board can have.
    const MAX_CELLS: usize = 1 << 20;

    pub fn serialize<S: Serializer>(cells: &Arc<[bool]>, s: S) -> Result<S::Ok, S::Error> {
        let mut bits = vec![0u8; cells.len().div_ceil(8)];
        for (i, _) in cells.iter().enumerate().filter(|(_, wall)| **wall) {
            bits[i / 8] |= 1 << (i % 8);
        }
        (cells.len() as u32, bits).serialize(s)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Arc<[bool]>, D::Error> {
        let (len, bits) = <(u32, Vec<u8>)>::deserialize(d)?;
        let len = len as usize;
        if len > MAX_CELLS || bits.len() != len.div_ceil(8) {
            return Err(D::Error::custom("walls of the wrong size"));
        }
        Ok((0..len)
            .map(|i| bits[i / 8] & (1 << (i % 8)) != 0)
            .collect())
    }
}

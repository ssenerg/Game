//! Game settings: the [`Rules`] that change how a game plays, and the
//! [`Theme`] that only changes how it looks.

/// Names of the selectable colors. The UI maps each index to a terminal
/// color, in this order.
pub const COLOR_NAMES: &[&str] = &[
    "red",
    "light red",
    "green",
    "light green",
    "yellow",
    "light yellow",
    "blue",
    "light blue",
    "magenta",
    "light magenta",
    "cyan",
    "light cyan",
    "white",
    "gray",
    "dark gray",
    "black",
];

pub fn color_name(idx: usize) -> &'static str {
    COLOR_NAMES[idx % COLOR_NAMES.len()]
}

/// Selectable icons for food and guns, as `(name, glyph)`. Only single-width
/// characters — the board renders each cell as the glyph plus a space. New
/// ones go last: settings keep icons by their place here.
pub const GLYPHS: &[(&str, &str)] = &[
    ("dot", "●"),
    ("circle", "○"),
    ("diamond", "◆"),
    ("diamond outline", "◇"),
    ("star", "★"),
    ("star outline", "☆"),
    ("heart", "♥"),
    ("square", "■"),
    ("square outline", "□"),
    ("triangle up", "▲"),
    ("triangle down", "▼"),
    ("cross", "✖"),
    ("plus", "✚"),
    ("asterisk", "*"),
    ("at sign", "@"),
    ("dollar", "$"),
    ("hash", "#"),
    ("percent", "%"),
    ("zigzag", "↯"),
    ("gun", "⌐"),
    ("circled plus", "⊕"),
    ("four-pointed star", "✦"),
    ("triple tilde", "≋"),
];

pub fn glyph_name(idx: usize) -> &'static str {
    GLYPHS[idx % GLYPHS.len()].0
}

pub fn glyph_char(idx: usize) -> &'static str {
    GLYPHS[idx % GLYPHS.len()].1
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum WallMode {
    Full,
    Open,
    Gaps,
}

impl WallMode {
    pub const ALL: [WallMode; 3] = [WallMode::Full, WallMode::Open, WallMode::Gaps];
    pub(crate) const LABELS: [&'static str; 3] =
        ["full_edge_wall", "no_edge_wall", "some_edge_wall"];
    pub(crate) const DISPLAY: [&'static str; 3] = ["solid", "none (wrap)", "gaps"];

    pub(crate) fn index(self) -> usize {
        Self::ALL.iter().position(|m| *m == self).unwrap()
    }

    pub fn label(self) -> &'static str {
        Self::LABELS[self.index()]
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum InnerWalls {
    Off,
    Few,
    Medium,
    Many,
}

impl InnerWalls {
    pub const ALL: [InnerWalls; 4] = [
        InnerWalls::Off,
        InnerWalls::Few,
        InnerWalls::Medium,
        InnerWalls::Many,
    ];
    pub(crate) const LABELS: [&'static str; 4] = ["off", "few", "medium", "many"];

    pub(crate) fn index(self) -> usize {
        Self::ALL.iter().position(|m| *m == self).unwrap()
    }

    pub fn label(self) -> &'static str {
        Self::LABELS[self.index()]
    }

    pub fn density_percent(self) -> usize {
        match self {
            InnerWalls::Off => 0,
            InnerWalls::Few => 6,
            InnerWalls::Medium => 12,
            InnerWalls::Many => 18,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum GrowthMode {
    Instant,
    AtTail,
}

impl GrowthMode {
    pub const ALL: [GrowthMode; 2] = [GrowthMode::Instant, GrowthMode::AtTail];
    pub(crate) const LABELS: [&'static str; 2] = ["instant", "at_tail"];
    pub(crate) const DISPLAY: [&'static str; 2] = ["instant", "when food reaches tail"];

    pub(crate) fn index(self) -> usize {
        Self::ALL.iter().position(|m| *m == self).unwrap()
    }

    pub fn label(self) -> &'static str {
        Self::DISPLAY[self.index()]
    }
}

/// When a direction key moves the snake.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum MoveTiming {
    /// Right away, ahead of the tick timer.
    Instant,
    /// On the next tick; keys pressed in between are queued.
    OnTick,
}

impl MoveTiming {
    pub const ALL: [MoveTiming; 2] = [MoveTiming::Instant, MoveTiming::OnTick];
    pub(crate) const LABELS: [&'static str; 2] = ["instant", "tick"];
    pub(crate) const DISPLAY: [&'static str; 2] = ["instant", "on tick"];

    pub(crate) fn index(self) -> usize {
        Self::ALL.iter().position(|m| *m == self).unwrap()
    }
}

#[derive(Clone, Copy)]
pub enum ColorSlot {
    Head,
    Body,
    Food,
    SuperFood,
    Poison,
    Wall,
    Background,
    /// A laser on the board, and its beam.
    Laser,
    /// A pistol on the board, and its bullets.
    Pistol,
    /// A grenade on the board, and its blast.
    Grenade,
    Flashbang,
    /// Smoke on the board, and its cloud.
    Smoke,
}

pub const COLOR_SLOTS: usize = 12;

#[derive(Clone, Copy)]
pub enum IconSlot {
    Food,
    SuperFood,
    Poison,
    Laser,
    Pistol,
    Grenade,
    Flashbang,
    Smoke,
}

pub const ICON_SLOTS: usize = 8;

impl IconSlot {
    /// The color slot this icon is drawn with, used for menu previews.
    pub fn color_slot(self) -> ColorSlot {
        match self {
            IconSlot::Food => ColorSlot::Food,
            IconSlot::SuperFood => ColorSlot::SuperFood,
            IconSlot::Poison => ColorSlot::Poison,
            IconSlot::Laser => ColorSlot::Laser,
            IconSlot::Pistol => ColorSlot::Pistol,
            IconSlot::Grenade => ColorSlot::Grenade,
            IconSlot::Flashbang => ColorSlot::Flashbang,
            IconSlot::Smoke => ColorSlot::Smoke,
        }
    }
}

/// Everything that changes how a game plays. Two games with equal rules
/// count toward the same record.
/// The fastest tick the game allows, in milliseconds.
pub const TICK_MS_MIN: u64 = 50;
/// The slowest.
pub const TICK_MS_MAX: u64 = 1000;
/// Settings, and a room's host, change the tick in steps of this.
pub const TICK_MS_STEP: u64 = 25;

#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Rules {
    pub width: u16,
    pub height: u16,
    pub tick_ms: u64,
    pub wall_mode: WallMode,
    pub inner_walls: InnerWalls,
    pub food_interval_s: u64,
    pub food_ttl_s: u64,
    pub super_interval_s: u64,
    pub super_ttl_s: u64,
    pub poison_interval_s: u64,
    pub poison_ttl_s: u64,
    pub growth_mode: GrowthMode,
    pub pass_through_self: bool,
    pub super_growth: u32,
    pub boost_duration_s: u64,
    pub move_timing: MoveTiming,
    /// Whether guns appear on the board. Only rooms have them: single
    /// player has nobody to shoot.
    pub guns: bool,
    pub gun_interval_s: u64,
    /// How long a gun stays on the board if nobody takes it.
    pub gun_ttl_s: u64,
    /// How long a snake that takes a gun has to fire it.
    pub gun_hold_s: u64,
    /// Cells a pistol's bullet moves each tick; snakes move one.
    pub bullet_speed: u64,
    /// Bots playing against you in single player. Rooms seat theirs apart.
    pub bots: u8,
    /// How well bots play, in single player and in rooms: one of
    /// [`crate::bot::LEVELS`].
    pub bot_level: u8,
    /// How often a thrown item appears, with guns on; 0 for never.
    pub thrown_interval_s: u64,
    /// How long it stays on the board if nobody takes it.
    pub thrown_ttl_s: u64,
    /// Cells a grenade's blast reaches from where it lands.
    pub grenade_radius: u16,
    /// How long a flashbang blinds.
    pub flash_s: u64,
    /// Cells a smoke cloud reaches from where it lands.
    pub smoke_radius: u16,
    /// How long it lasts.
    pub smoke_s: u64,
}

impl Default for Rules {
    fn default() -> Self {
        Self {
            width: 30,
            height: 20,
            tick_ms: 150,
            wall_mode: WallMode::Full,
            inner_walls: InnerWalls::Off,
            food_interval_s: 3,
            food_ttl_s: 15,
            super_interval_s: 20,
            super_ttl_s: 8,
            poison_interval_s: 15,
            poison_ttl_s: 12,
            growth_mode: GrowthMode::Instant,
            pass_through_self: false,
            super_growth: 3,
            boost_duration_s: 5,
            move_timing: MoveTiming::Instant,
            guns: false,
            gun_interval_s: 10,
            gun_ttl_s: 8,
            gun_hold_s: 5,
            bullet_speed: 3,
            bots: 0,
            bot_level: 3,
            thrown_interval_s: 15,
            thrown_ttl_s: 8,
            grenade_radius: 2,
            flash_s: 3,
            smoke_radius: 3,
            smoke_s: 8,
        }
    }
}

impl Rules {
    /// Identifies these rules in the records file. The format is stored on
    /// disk, so changing it orphans existing records. Settings added later
    /// are appended only when they differ from the old behaviour. Guns
    /// aren't: records are single player's, which has none.
    pub fn record_key(&self) -> String {
        let mut key = format!(
            "size:{}x{} tick:{} edge:{} inner:{} food:{}/{} super:{}/{} poison:{}/{} growth:{} ghost:{} supergrow:{} boost:{}",
            self.width,
            self.height,
            self.tick_ms,
            self.wall_mode.label(),
            self.inner_walls.label(),
            self.food_interval_s,
            self.food_ttl_s,
            self.super_interval_s,
            self.super_ttl_s,
            self.poison_interval_s,
            self.poison_ttl_s,
            self.growth_mode.label(),
            self.pass_through_self,
            self.super_growth,
            self.boost_duration_s,
        );
        if self.move_timing == MoveTiming::OnTick {
            key.push_str(" timing:tick");
        }
        if self.bots > 0 {
            key.push_str(&format!(" bots:{}", self.bots));
            // Bots played as well as they could before they had levels.
            if self.bot_level != *crate::bot::LEVELS.end() {
                key.push_str(&format!(" level:{}", self.bot_level));
            }
            if self.guns {
                key.push_str(&format!(
                    " guns:{}/{}/{}/{}",
                    self.gun_interval_s, self.gun_ttl_s, self.gun_hold_s, self.bullet_speed
                ));
                if self.thrown_interval_s > 0 {
                    key.push_str(&format!(
                        " thrown:{}/{} blast:{} flash:{} smoke:{}/{}",
                        self.thrown_interval_s,
                        self.thrown_ttl_s,
                        self.grenade_radius,
                        self.flash_s,
                        self.smoke_radius,
                        self.smoke_s
                    ));
                }
            }
        }
        key
    }

    /// Single player's guns need someone to shoot: with guns on, at least
    /// one bot plays.
    pub fn bot_for_guns(&mut self) {
        if self.guns && self.bots == 0 {
            self.bots = 1;
        }
    }

    /// These rules as single player plays them. With bots, every snake
    /// moves on the tick and super food gives no boost, as in rooms.
    pub fn for_solo(mut self) -> Rules {
        self.bot_for_guns();
        if self.bots > 0 {
            self.move_timing = MoveTiming::OnTick;
            self.boost_duration_s = 0;
        }
        self
    }
}

/// How the game looks: icon and color choices, as indices into [`GLYPHS`]
/// and [`COLOR_NAMES`]. Never affects records.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Theme {
    pub icons: [usize; ICON_SLOTS],
    pub colors: [usize; COLOR_SLOTS],
}

impl Default for Theme {
    fn default() -> Self {
        Self {
            // food ●, super food ★, poison ✖, laser ↯, pistol ⌐, grenade ⊕,
            // flashbang ✦, smoke ≋
            icons: [0, 4, 11, 18, 19, 20, 21, 22],
            // head, body, food, super food, poison, wall, background, laser,
            // pistol, grenade, flashbang, smoke
            colors: [3, 2, 1, 5, 8, 13, 15, 11, 12, 0, 4, 13],
        }
    }
}

impl Theme {
    pub fn icon(&self, slot: IconSlot) -> &'static str {
        glyph_char(self.icons[slot as usize])
    }

    pub fn color_index(&self, slot: ColorSlot) -> usize {
        self.colors[slot as usize] % COLOR_NAMES.len()
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GameConfig {
    pub rules: Rules,
    pub theme: Theme,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_record_key_is_unchanged() {
        // Saved records are keyed by this string; it must never drift.
        assert_eq!(
            Rules::default().record_key(),
            "size:30x20 tick:150 edge:full_edge_wall inner:off food:3/15 super:20/8 \
             poison:15/12 growth:instant ghost:false supergrow:3 boost:5"
        );
    }

    #[test]
    fn on_tick_games_have_their_own_records() {
        let on_tick = Rules {
            move_timing: MoveTiming::OnTick,
            ..Rules::default()
        };
        assert_eq!(
            on_tick.record_key(),
            Rules::default().record_key() + " timing:tick"
        );
    }

    #[test]
    fn games_against_bots_have_their_own_records() {
        let guns = Rules {
            guns: true,
            ..Rules::default()
        };
        assert_eq!(
            guns.record_key(),
            Rules::default().record_key(),
            "alone, there are no guns to play with"
        );
        let solo = guns.for_solo();
        assert_eq!(solo.bots, 1, "guns need someone to shoot");
        assert_eq!(
            (solo.move_timing, solo.boost_duration_s),
            (MoveTiming::OnTick, 0),
            "as rooms play"
        );
        assert!(
            solo.record_key().ends_with(
                " timing:tick bots:1 level:3 guns:10/8/5/3 thrown:15/8 blast:2 flash:3 smoke:3/8"
            ),
            "{}",
            solo.record_key()
        );
        let best = Rules {
            bot_level: 5,
            ..solo.clone()
        };
        assert_eq!(
            best.record_key(),
            solo.record_key().replace(" level:3", ""),
            "bots at their best keep the records from before levels"
        );
        assert_eq!(Rules::default().for_solo(), Rules::default());
    }
}

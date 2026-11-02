use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::Span;
use ratatui::widgets::Widget;
use shell_tui::ui::{dim, secs};
use snake_core::config::{ColorSlot, IconSlot, Theme};
use snake_core::geom::{Direction, Point};
use snake_core::guns::Item;
use snake_core::view::{MatchView, SnakeView};
use snake_core::world::FoodKind;

use crate::palette::theme_color;

const BLOCK: [&str; 2] = ["█", "█"];
const EMPTY: [&str; 2] = [" ", " "];
const DIGITS: [&str; 10] = ["0", "1", "2", "3", "4", "5", "6", "7", "8", "9"];
/// A laser's beam fills both columns of a cell going across, and the first
/// going up or down, so it reads as one line.
const BEAM_ACROSS: [&str; 2] = ["━", "━"];
const BEAM_DOWN: [&str; 2] = ["┃", " "];
const MARK: [&str; 2] = ["◎", " "];
const BURST: [&str; 2] = ["✸", " "];
const SMOKE: [&str; 2] = ["░", "░"];
/// What a flashbang leaves its opponents to see.
const FLASH: Color = Color::White;

/// How an item shows in this player's theme, on the board and beside it.
/// Its shots, marks, blasts and clouds take its color.
pub fn item_look(theme: &Theme, kind: Item) -> (&'static str, Color) {
    let (icon, color) = match kind {
        Item::Laser => (IconSlot::Laser, ColorSlot::Laser),
        Item::Pistol => (IconSlot::Pistol, ColorSlot::Pistol),
        Item::Grenade => (IconSlot::Grenade, ColorSlot::Grenade),
        Item::Flashbang => (IconSlot::Flashbang, ColorSlot::Flashbang),
        Item::Smoke => (IconSlot::Smoke, ColorSlot::Smoke),
    };
    (theme.icon(icon), theme_color(theme, color))
}

/// What a status line says of the player's snake, if anything: that a
/// flashbang blinds it, and of its item. `releases` says whether their
/// terminal says when `f` is let go.
pub fn snake_hints(theme: &Theme, snake: &SnakeView, releases: bool) -> Vec<Span<'static>> {
    let mut spans = Vec::new();
    for hint in [blind_hint(snake), item_hint(theme, snake, releases)]
        .into_iter()
        .flatten()
    {
        if !spans.is_empty() {
            spans.push(Span::styled(" · ", dim()));
        }
        spans.push(hint);
    }
    spans
}

/// What the item is, how to use it and how long is left; or, on its way,
/// how to throw it.
fn item_hint(theme: &Theme, snake: &SnakeView, releases: bool) -> Option<Span<'static>> {
    let (kind, how) = match (snake.throwing, snake.held) {
        (Some(kind), _) if releases => (kind, "let go of f to throw".to_string()),
        (Some(kind), _) => (kind, "f again to throw".to_string()),
        (None, Some((kind, left))) => {
            let how = if !kind.thrown() {
                "f fire"
            } else if releases {
                "hold f to aim"
            } else {
                "f to aim"
            };
            (kind, format!("{how} · {} s", secs(left).max(1)))
        }
        (None, None) => return None,
    };
    let (icon, color) = item_look(theme, kind);
    Some(Span::styled(
        format!("{icon} {} · {how}", kind.name()),
        Style::default().fg(color).add_modifier(Modifier::BOLD),
    ))
}

fn blind_hint(snake: &SnakeView) -> Option<Span<'static>> {
    let left = snake.blind?;
    Some(Span::styled(
        format!("Blinded · {} s", secs(left).max(1)),
        Style::default()
            .fg(Color::Black)
            .bg(FLASH)
            .add_modifier(Modifier::BOLD),
    ))
}

/// How one snake is drawn in multiplayer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SnakeLook {
    pub head: Color,
    pub body: Color,
    /// Drawn on the head, so color isn't the only way to tell snakes apart.
    pub number: Option<u8>,
}

/// Draws a board straight into the buffer, two terminal columns per cell.
/// From the bottom: walls, food, items, bodies, bullets, beams, blasts and
/// marks; then heads, so a player never loses sight of theirs, and smoke
/// over everything.
pub struct BoardWidget<'a> {
    pub view: &'a MatchView,
    pub theme: &'a Theme,
    /// The board cell drawn at the area's top-left corner, for boards bigger
    /// than the area.
    pub origin: Point,
    /// Per snake id. Without it, every snake gets the theme's colors.
    pub looks: Option<&'a [SnakeLook]>,
    /// A flashbang blinds the player: all they see is white.
    pub blind: bool,
}

impl<'a> BoardWidget<'a> {
    pub fn new(view: &'a MatchView, theme: &'a Theme) -> Self {
        Self {
            view,
            theme,
            origin: Point { x: 0, y: 0 },
            looks: None,
            blind: false,
        }
    }
}

impl Widget for BoardWidget<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let Self {
            view,
            theme,
            origin,
            looks,
            blind,
        } = self;
        let bg = theme_color(theme, ColorSlot::Background);
        let mut paint = |p: Point, text: [&str; 2], style: Style| {
            if p.x < origin.x || p.y < origin.y {
                return;
            }
            let y = u32::from(area.y) + u32::from(p.y - origin.y);
            for (i, symbol) in text.into_iter().enumerate() {
                let x = u32::from(area.x) + u32::from(p.x - origin.x) * 2 + i as u32;
                if x >= u32::from(area.right()) || y >= u32::from(area.bottom()) {
                    continue;
                }
                if let Some(cell) = buf.cell_mut((x as u16, y as u16)) {
                    cell.reset();
                    cell.set_symbol(symbol).set_style(style);
                }
            }
        };
        let plain = |fg: Color| Style::default().fg(fg).bg(bg);
        if blind {
            for y in 0..view.height {
                for x in 0..view.width {
                    paint(Point { x, y }, EMPTY, Style::default().bg(FLASH));
                }
            }
            return;
        }

        let wall = theme_color(theme, ColorSlot::Wall);
        for y in 0..view.height {
            for x in 0..view.width {
                let p = Point { x, y };
                if view.wall_at(x, y) {
                    paint(p, BLOCK, plain(wall));
                } else {
                    paint(p, EMPTY, plain(bg));
                }
            }
        }
        for &(p, kind) in &view.foods {
            let (icon, color) = match kind {
                FoodKind::Normal => (IconSlot::Food, ColorSlot::Food),
                FoodKind::Super => (IconSlot::SuperFood, ColorSlot::SuperFood),
                FoodKind::Poison => (IconSlot::Poison, ColorSlot::Poison),
            };
            // The glyph, padded to the cell's two columns.
            let style = plain(theme_color(theme, color)).add_modifier(Modifier::BOLD);
            paint(p, [theme.icon(icon), " "], style);
        }
        for &(p, kind) in &view.items {
            let (icon, color) = item_look(theme, kind);
            paint(p, [icon, " "], plain(color).add_modifier(Modifier::BOLD));
        }
        let look = |id: u8| {
            looks
                .and_then(|l| l.get(id as usize).copied())
                .unwrap_or(SnakeLook {
                    head: theme_color(theme, ColorSlot::Head),
                    body: theme_color(theme, ColorSlot::Body),
                    number: None,
                })
        };
        for snake in &view.snakes {
            let body = look(snake.id).body;
            for &p in snake.body.iter().skip(1) {
                paint(p, BLOCK, plain(body));
            }
        }
        for &(p, _) in &view.bullets {
            paint(
                p,
                ["•", " "],
                plain(item_look(theme, Item::Pistol).1).add_modifier(Modifier::BOLD),
            );
        }
        let beam = item_look(theme, Item::Laser).1;
        for (dir, cells) in &view.beams {
            let line = match dir {
                Direction::Left | Direction::Right => BEAM_ACROSS,
                Direction::Up | Direction::Down => BEAM_DOWN,
            };
            for &p in cells {
                paint(p, line, plain(beam).add_modifier(Modifier::BOLD));
            }
        }
        for (kind, cells) in &view.bursts {
            let style = plain(item_look(theme, *kind).1).add_modifier(Modifier::BOLD);
            for &p in cells {
                paint(p, BURST, style);
            }
        }
        for &(p, kind) in &view.marks {
            paint(
                p,
                MARK,
                plain(item_look(theme, kind).1).add_modifier(Modifier::BOLD),
            );
        }
        for snake in &view.snakes {
            let Some(&p) = snake.body.first() else {
                continue;
            };
            let look = look(snake.id);
            match look.number {
                Some(n) => {
                    let style = Style::default()
                        .fg(Color::Black)
                        .bg(look.head)
                        .add_modifier(Modifier::BOLD);
                    paint(p, [DIGITS[n as usize % 10], " "], style);
                }
                None => paint(p, BLOCK, plain(look.head)),
            }
        }
        let smoke = plain(item_look(theme, Item::Smoke).1);
        for &p in &view.smoke {
            paint(p, SMOKE, smoke);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    fn said(snake: &SnakeView, releases: bool) -> String {
        snake_hints(&Theme::default(), snake, releases)
            .iter()
            .map(|s| s.content.as_ref())
            .collect()
    }

    #[test]
    fn hints_say_how_to_throw_by_what_the_terminal_can_say() {
        let mut snake = SnakeView {
            id: 0,
            body: vec![Point { x: 1, y: 1 }],
            alive: true,
            held: Some((Item::Smoke, Duration::from_millis(1200))),
            throwing: None,
            blind: None,
        };
        assert_eq!(said(&snake, false), "≋ smoke · f to aim · 2 s");
        assert_eq!(said(&snake, true), "≋ smoke · hold f to aim · 2 s");
        snake.held = None;
        snake.throwing = Some(Item::Smoke);
        assert_eq!(said(&snake, false), "≋ smoke · f again to throw");
        assert_eq!(said(&snake, true), "≋ smoke · let go of f to throw");
        snake.throwing = None;
        snake.blind = Some(Duration::from_millis(300));
        assert_eq!(said(&snake, true), "Blinded · 1 s");
    }
}

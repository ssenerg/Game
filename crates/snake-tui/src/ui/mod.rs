//! Snake's own screens: single player, a room's rules, and its rounds.

mod board;
mod dialogs;
mod game;
mod menu;
mod room;
mod room_rules;

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::text::Line;
use ratatui::widgets::{Clear, Paragraph};
use shell_tui::Ctx;

use crate::app::{SnakeScreen, SnakeUi};

pub use board::{BoardWidget, SnakeLook, item_look};
pub use game::{board_fits, board_needs};
pub(crate) use menu::LOGO;
pub(crate) use room::draw_round;
pub use room::{room_fits, room_size};

pub(crate) fn draw(frame: &mut Frame, ui: &SnakeUi, ctx: &Ctx) {
    match ui.screen {
        SnakeScreen::Menu => menu::draw(frame, ui, ctx.online),
        SnakeScreen::Playing => game::draw(frame, ui, ctx.key_releases),
        SnakeScreen::Paused { selected } => {
            game::draw(frame, ui, ctx.key_releases);
            dialogs::draw_pause(frame, ui, selected);
        }
        SnakeScreen::GameOver { new_record } => {
            game::draw(frame, ui, ctx.key_releases);
            dialogs::draw_game_over(frame, ui, new_record);
        }
        SnakeScreen::RoomRules { selected } => room_rules::draw(frame, ui, ctx, selected),
        SnakeScreen::QuickPlay { selected } => dialogs::draw_quick_play(frame, ui, selected),
    }
    // What the app has to say, such as that an idle player is about to be
    // logged out, along the bottom; the rules show it their own way, and a
    // game in play keeps its board whole.
    if let Some(notice) = ctx.notice
        && !matches!(
            ui.screen,
            SnakeScreen::Playing | SnakeScreen::RoomRules { .. }
        )
    {
        let area = frame.area();
        if area.height > 0 {
            let row = Rect {
                y: area.bottom() - 1,
                height: 1,
                ..area
            };
            frame.render_widget(Clear, row);
            frame.render_widget(
                Paragraph::new(
                    Line::styled(notice.to_string(), Style::default().fg(Color::LightYellow))
                        .centered(),
                ),
                row,
            );
        }
    }
}

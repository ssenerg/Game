//! Over a single-player game: paused, and over. And quick play's choice of
//! pools.

use ratatui::Frame;
use ratatui::layout::Alignment;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use shell_tui::ui::dialog;
use snake_core::round::quick_rules;
use snake_core::solo::GameOverReason;

use super::room::{room_fits, room_size};
use crate::app::{QUICK_POOLS, SnakeUi};

pub fn draw_quick_play(frame: &mut Frame, app: &SnakeUi, selected: usize) {
    // Every pool plays on the same board.
    let rules = quick_rules();
    let too_small = !room_fits(rules.width, rules.height, app.size);
    let inner = dialog(
        frame,
        " QUICK PLAY ",
        Color::Cyan,
        48,
        8 + 2 * u16::from(too_small),
    );
    let dim = Style::default().fg(Color::DarkGray);
    let mut lines = vec![Line::default()];
    for (i, (_, name, about)) in QUICK_POOLS.iter().enumerate() {
        let (mark, style) = if i == selected {
            (
                "❯ ",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            )
        } else {
            ("  ", Style::default())
        };
        lines.push(Line::from(vec![
            Span::styled(format!("{mark}{name:<9}"), style),
            Span::styled(format!("{about:<31}"), dim),
        ]));
    }
    lines.push(Line::default());
    if too_small {
        let (w, h) = room_size(rules.width, rules.height);
        lines.push(Line::styled(
            format!(
                "Your terminal is {}×{}; matches need {w}×{h}",
                app.size.width, app.size.height
            ),
            Style::default().fg(Color::Yellow),
        ));
        lines.push(Line::styled(
            "to show the whole board.",
            Style::default().fg(Color::Yellow),
        ));
    }
    lines.push(Line::styled("↑↓ + Enter · Esc back", dim));
    frame.render_widget(Paragraph::new(lines).alignment(Alignment::Center), inner);
}

pub fn draw_pause(frame: &mut Frame, selected: usize) {
    let inner = dialog(frame, " PAUSED ", Color::Yellow, 36, 8);

    let item = |label: &str, active: bool| {
        if active {
            Line::from(format!("❯ {label}")).style(
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            )
        } else {
            Line::from(format!("  {label}"))
        }
    };
    let lines = vec![
        Line::default(),
        item("Resume", selected == 0),
        item("Quit to menu", selected == 1),
        Line::default(),
        Line::from("↑↓ + Enter · Esc resume · q menu").style(Style::default().fg(Color::DarkGray)),
    ];
    frame.render_widget(Paragraph::new(lines).alignment(Alignment::Center), inner);
}

pub fn draw_game_over(frame: &mut Frame, app: &SnakeUi, new_record: bool) {
    let Some(game) = &app.game else { return };
    let (reason, color) = match game.over {
        Some(GameOverReason::HitWall) => ("You crashed into a wall!", Color::LightRed),
        Some(GameOverReason::HitSelf) => ("You bit yourself!", Color::LightRed),
        Some(GameOverReason::Poisoned) => ("✖ You ate poison!", Color::LightMagenta),
        Some(GameOverReason::MaxLength) => ("★ Board complete — you win! ★", Color::LightGreen),
        None => ("", Color::White),
    };
    let title = if matches!(game.over, Some(GameOverReason::MaxLength)) {
        " YOU WIN "
    } else {
        " GAME OVER "
    };
    let inner = dialog(frame, title, color, 46, 11);

    let dim = Style::default().fg(Color::DarkGray);
    let secs = game.clock().as_secs();
    let mut lines = vec![
        Line::default(),
        Line::from(reason).style(Style::default().fg(color).add_modifier(Modifier::BOLD)),
        Line::default(),
        Line::from(format!(
            "Length {}    Time {:02}:{:02}",
            game.len(),
            secs / 60,
            secs % 60
        )),
        Line::from(format!("★ Best {}", app.record)),
    ];
    if new_record {
        lines.push(
            Line::from("★ NEW RECORD ★").style(
                Style::default()
                    .fg(Color::LightYellow)
                    .add_modifier(Modifier::BOLD),
            ),
        );
    }
    lines.push(Line::default());
    lines.push(Line::from("r play again · Enter menu · q quit").style(dim));
    frame.render_widget(Paragraph::new(lines).alignment(Alignment::Center), inner);
}

//! The signed-in account: rating, remembered machines, password, sign out.

use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Paragraph, Wrap};

use super::{centered_rect, draw_logo};
use crate::app::App;
use crate::online::AccountRow;

pub fn draw(frame: &mut Frame, app: &App, selected: usize) {
    let online = &app.online;
    let Some(profile) = &online.account else {
        return;
    };
    let rows = online.account_rows();
    let categories = app.ui().ratings();
    let height = rows.len() + categories.len() + 8;
    // The logo goes when it would push rows off the screen.
    let logo = if usize::from(frame.area().height) >= height + 4 + 3 {
        4
    } else {
        0
    };
    let [logo_area, body_area, footer_area] = Layout::vertical([
        Constraint::Length(logo),
        Constraint::Min(3),
        Constraint::Length(3),
    ])
    .areas(frame.area());
    if logo > 0 {
        draw_logo(frame, app, logo_area);
    }

    let rect = centered_rect(body_area, 64, height as u16);
    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(super::dim())
        .title(format!(" {} ", profile.username))
        .title_alignment(Alignment::Center);
    let inner = block.inner(rect);
    frame.render_widget(block, rect);

    let dim = super::dim();
    let bold = Style::default().add_modifier(Modifier::BOLD);
    let mut lines = vec![Line::default()];
    // The open game's ratings, each kept apart.
    for (i, category) in categories.iter().enumerate() {
        let Some(shown) = profile.rating(app.rating_key(i)) else {
            continue;
        };
        lines.push(Line::from(vec![
            Span::styled("  Rating ", dim),
            Span::styled(shown.text(), bold.fg(Color::LightCyan)),
            Span::styled(
                format!(
                    " · {} games · {} wins in {}",
                    shown.games,
                    shown.wins,
                    category.label.to_lowercase()
                ),
                dim,
            ),
        ]));
    }
    lines.push(Line::default());
    lines.push(Line::styled(
        "  Machines that sign you in by themselves",
        dim,
    ));
    if online.keys.is_empty() {
        lines.push(Line::styled("    none", dim));
    }
    // The selected row's line, which scrolling keeps in view.
    let mut shown = 0;
    for (i, row) in rows.iter().enumerate() {
        let active = i == selected;
        if active {
            // Change password has a blank line above it.
            shown = lines.len() + usize::from(*row == AccountRow::ChangePassword);
        }
        let style = if active {
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default()
        };
        let marker = if active { "  ❯ " } else { "    " };
        match row {
            AccountRow::Key(k) => {
                let key = &online.keys[*k];
                let short: String = key.fingerprint.chars().take(20).collect();
                let mut spans = vec![
                    Span::styled(format!("{marker}{short}…"), style),
                    Span::styled(format!("  {}", key.last_used), dim),
                ];
                if key.this_machine {
                    spans.push(Span::styled(
                        "  this machine",
                        Style::default().fg(Color::Cyan),
                    ));
                }
                lines.push(Line::from(spans));
            }
            AccountRow::ChangePassword => {
                lines.push(Line::default());
                lines.push(Line::styled(format!("{marker}Change password"), style));
            }
            AccountRow::SignOut => lines.push(Line::styled(
                format!("{marker}Sign out on this machine"),
                style,
            )),
            AccountRow::Back => lines.push(Line::styled(format!("{marker}Back"), style)),
        }
    }
    let fits = usize::from(inner.height).max(1);
    let scroll = (shown + 1).saturating_sub(fits) as u16;
    frame.render_widget(Paragraph::new(lines).scroll((scroll, 0)), inner);

    let status = match &online.notice {
        Some(notice) => Line::styled(
            notice.clone(),
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ),
        None => Line::default(),
    };
    let footer = vec![
        status,
        Line::styled(
            "↑↓ select · Enter choose · x forget a machine · Esc back",
            dim,
        ),
    ];
    frame.render_widget(
        Paragraph::new(footer)
            .alignment(Alignment::Center)
            .wrap(Wrap { trim: true }),
        footer_area,
    );
}

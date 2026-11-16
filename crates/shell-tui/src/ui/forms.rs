//! Signing up, signing in, and changing a password.

use ratatui::Frame;
use ratatui::layout::{Alignment, Margin};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Wrap};

use super::dialog;
use crate::app::App;
use crate::online::{FormKind, FormRow};

pub fn draw(frame: &mut Frame, app: &App, kind: FormKind) {
    let has_key = app.online.has_key;
    let rows = kind.rows(has_key);
    let form = &app.form;
    let dim = super::dim();
    let field = |label: &str, value: String, focused: bool| {
        let (marker, style) = if focused {
            (
                "❯ ",
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            )
        } else {
            ("  ", Style::default())
        };
        let cursor = if focused { "▏" } else { "" };
        Line::from(vec![
            Span::styled(format!("{marker}{label:<17}"), style),
            Span::styled(format!("{value}{cursor}"), style.fg(Color::LightYellow)),
        ])
    };
    let masked = |len: usize| "•".repeat(len);

    let mut lines = vec![Line::default()];
    for (i, row) in rows.iter().enumerate() {
        let focused = i == form.focus;
        lines.push(match row {
            FormRow::Username => field("Username", form.username.clone(), focused),
            FormRow::Current => field("Current password", masked(form.current.len()), focused),
            FormRow::Password => {
                let label = if kind == FormKind::ChangePassword {
                    "New password"
                } else {
                    "Password"
                };
                field(label, masked(form.password.len()), focused)
            }
            FormRow::Repeat => field("Repeat password", masked(form.repeat.len()), focused),
            FormRow::Remember => {
                let mark = if form.remember { "[x]" } else { "[ ]" };
                field(mark, "Remember this machine".to_string(), focused)
            }
        });
    }
    lines.push(Line::default());
    lines.push(match (&form.error, form.busy) {
        (_, true) => Line::from("One moment…"),
        (Some(error), _) => Line::styled(error.clone(), Style::default().fg(Color::LightRed)),
        (None, _) => Line::default(),
    });
    lines.push(Line::default());
    let note = match kind {
        FormKind::SignUp if has_key => {
            "Usernames: 3 to 16 letters, digits, _ or -. Passwords: 8 or more \
             characters, just for SnakeTUI, not your computer's. This machine \
             will sign you in by itself from now on."
        }
        FormKind::SignUp => {
            "Usernames: 3 to 16 letters, digits, _ or -. Passwords: 8 or more \
             characters, just for SnakeTUI, not your computer's."
        }
        FormKind::ChangePassword => {
            "8 or more characters, just for SnakeTUI. Other machines you signed \
             in on will need it again."
        }
        FormKind::SignIn => "Your SnakeTUI username and password.",
    };
    lines.push(Line::styled(note, dim));
    lines.push(Line::default());
    // Enter moves on, or on the last row sends the form.
    let enter = if form.focus + 1 < rows.len() {
        "Enter next"
    } else {
        match kind {
            FormKind::SignUp => "Enter sign up",
            FormKind::SignIn => "Enter sign in",
            FormKind::ChangePassword => "Enter change it",
        }
    };
    let hint = if rows.contains(&FormRow::Remember) {
        format!("↑↓ move · Space tick · {enter} · Esc back")
    } else {
        format!("↑↓ move · {enter} · Esc back")
    };
    lines.push(Line::styled(hint, dim));
    // As tall as its lines once wrapped, so the hint is never cut off.
    let height: usize = lines.iter().map(|line| rows_for(line, TEXT_W)).sum();
    let inner = dialog(
        frame,
        kind.title(),
        Color::Cyan,
        TEXT_W as u16 + 6,
        height as u16 + 2,
    )
    .inner(Margin::new(2, 0));
    frame.render_widget(
        Paragraph::new(lines)
            .alignment(Alignment::Left)
            .wrap(Wrap { trim: false }),
        inner,
    );
}

/// The width the form's text wraps at.
const TEXT_W: usize = 50;

/// The rows `line` takes in a paragraph `width` wide that wraps by words.
fn rows_for(line: &Line, width: usize) -> usize {
    if line.width() <= width {
        return 1;
    }
    let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
    let (mut rows, mut used) = (1, 0);
    for word in text.split(' ') {
        let len = word.chars().count();
        if used > 0 && used + 1 + len <= width {
            used += 1 + len;
        } else if used == 0 && len <= width {
            used = len;
        } else {
            // On a row of its own, split if it's longer than a row.
            rows += usize::from(used > 0) + len.saturating_sub(1) / width;
            used = if len == 0 { 0 } else { (len - 1) % width + 1 };
        }
    }
    rows
}

#[cfg(test)]
mod tests {
    use ratatui::buffer::Buffer;
    use ratatui::layout::Rect;
    use ratatui::widgets::Widget;

    use super::*;

    /// The rows `text` takes once a paragraph `width` wide draws it.
    fn drawn_rows(text: &str, width: u16) -> usize {
        let area = Rect::new(0, 0, width, 200);
        let mut buffer = Buffer::empty(area);
        Paragraph::new(text.to_string())
            .wrap(Wrap { trim: false })
            .render(area, &mut buffer);
        let blank = |y| (0..width).all(|x| buffer[(x, y)].symbol() == " ");
        (0..200)
            .rev()
            .find(|&y| !blank(y))
            .map_or(1, |y| y as usize + 1)
    }

    #[test]
    fn rows_are_counted_as_a_paragraph_wraps_them() {
        let texts = [
            "one two",
            "one two three",
            "abcdefghijklmnop",
            "a b c d e f",
            "Usernames: 3 to 16 letters, digits, _ or -. Passwords: 8 or more \
             characters, just for SnakeTUI, not your computer's. This machine \
             will sign you in by itself from now on.",
            "↑↓ move · Space tick · Enter sign up · Esc back",
        ];
        for text in texts {
            for width in [3, 5, 6, 7, 20, 50] {
                assert_eq!(
                    rows_for(&Line::from(text), width),
                    drawn_rows(text, width as u16),
                    "{text:?} at {width}"
                );
            }
        }
    }
}

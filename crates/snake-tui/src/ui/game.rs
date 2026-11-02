use ratatui::Frame;
use ratatui::layout::{Alignment, Size};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Padding, Paragraph};
use snake_core::config::ColorSlot;
use snake_core::solo::SoloGame;
use snake_core::view::SnakeView;

use shell_tui::ui::centered_rect;

use super::board::{BoardWidget, SnakeLook, snake_hints};
use crate::app::SnakeUi;
use crate::palette::{opponent_colors, theme_color};

const PAD_X: u16 = 2;
const PAD_Y: u16 = 1;

/// Terminal size a `width`×`height` board needs: two columns per cell, plus
/// border and padding.
fn board_size(width: u16, height: u16) -> (u16, u16) {
    (width * 2 + 2 + 2 * PAD_X, height + 2 + 2 * PAD_Y)
}

/// Whether the board is drawn at all; below this size only a warning is shown.
pub fn board_fits(width: u16, height: u16, size: Size) -> bool {
    let (need_w, need_h) = board_size(width, height);
    size.width >= need_w && size.height >= need_h
}

/// Draws the game. `releases` says whether the player's terminal says when
/// keys are let go.
pub fn draw(frame: &mut Frame, app: &SnakeUi, releases: bool) {
    let Some(game) = &app.game else { return };
    let area = frame.area();

    let (width, height) = (game.world.width, game.world.height);
    let (need_w, need_h) = board_size(width, height);
    if !board_fits(width, height, area.as_size()) {
        frame.render_widget(
            Paragraph::new(format!(
                "terminal too small — need at least {need_w}x{need_h}"
            ))
            .alignment(Alignment::Center)
            .style(Style::default().fg(Color::Red)),
            area,
        );
        return;
    }

    let boost = game.boost_active();
    let border = if boost {
        Color::Yellow
    } else {
        Color::DarkGray
    };
    let view = game.world.view();
    // The player's own snake comes first.
    let me = view.snakes.first();
    let rect = centered_rect(area, need_w, need_h);
    let block = Block::bordered()
        .border_type(BorderType::Double)
        .border_style(Style::default().fg(border))
        .padding(Padding::new(PAD_X, PAD_X, PAD_Y, PAD_Y))
        .title_top(title_line())
        .title_top(stats_line(app, game, need_w).right_aligned())
        .title_bottom(hint_line(app, game, me, releases).centered());
    let inner = block.inner(rect);
    frame.render_widget(block, rect);
    let looks = looks(app, game);
    let mut board = BoardWidget::new(&view, &app.cfg.theme);
    if game.rules().bots > 0 {
        board.looks = Some(&looks);
    }
    board.blind = me.is_some_and(|s| s.blind.is_some());
    frame.render_widget(board, inner);
}

/// The player's snake in their own colors, and each bot in one apart from
/// them, with its number on its head.
fn looks(app: &SnakeUi, game: &SoloGame) -> Vec<SnakeLook> {
    let theme = &app.cfg.theme;
    let others = opponent_colors(theme);
    let mut looks = vec![SnakeLook {
        head: theme_color(theme, ColorSlot::Head),
        body: theme_color(theme, ColorSlot::Body),
        number: None,
    }];
    looks.extend((1..game.world.snakes.len()).map(|id| {
        let color = others[(id - 1) % others.len()];
        SnakeLook {
            head: color,
            body: color,
            number: Some(id as u8),
        }
    }));
    looks
}

fn title_line() -> Line<'static> {
    Line::styled(
        " S N A K E ",
        Style::default()
            .fg(Color::LightGreen)
            .add_modifier(Modifier::BOLD),
    )
    .left_aligned()
}

fn stats_line(app: &SnakeUi, game: &SoloGame, board_w: u16) -> Line<'static> {
    let dim = Style::default().fg(Color::DarkGray);
    let bold = Style::default().add_modifier(Modifier::BOLD);
    let len = game.len();
    let record = app.record.max(len as u32);
    let secs = game.clock().as_secs();
    let time = format!("{:02}:{:02}", secs / 60, secs % 60);

    if board_w < 52 {
        return Line::from(vec![
            Span::styled(format!(" {len} "), bold),
            Span::styled("★", Style::default().fg(Color::Yellow)),
            Span::styled(format!("{record} "), bold),
            Span::styled(format!("{time} "), bold),
        ]);
    }
    let mut line = vec![
        Span::styled(" Length ", dim),
        Span::styled(len.to_string(), bold),
        Span::styled(" · ", dim),
        Span::styled("★ ", Style::default().fg(Color::Yellow)),
        Span::styled("Best ", dim),
        Span::styled(record.to_string(), bold),
        Span::styled(" · ", dim),
        Span::styled(time, bold),
        Span::raw(" "),
    ];
    if game.rules().bots > 0 {
        let left = game.bots_left();
        let s = if left == 1 { "" } else { "s" };
        line.insert(0, Span::styled(format!(" {left} bot{s} ·"), dim));
    }
    Line::from(line)
}

fn hint_line(
    app: &SnakeUi,
    game: &SoloGame,
    me: Option<&SnakeView>,
    releases: bool,
) -> Line<'static> {
    let hints = me
        .filter(|_| game.over.is_none())
        .map_or_else(Vec::new, |s| snake_hints(&app.cfg.theme, s, releases));
    if !hints.is_empty() {
        let pad = || Span::raw(" ");
        Line::from([vec![pad()], hints, vec![pad()]].concat())
    } else if !game.started {
        Line::styled(
            " press an arrow key to start ",
            Style::default()
                .fg(Color::LightYellow)
                .add_modifier(Modifier::BOLD),
        )
    } else if game.boost_active() {
        Line::styled(
            " ⚡ BOOST ×2 ",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )
    } else {
        Line::styled(
            " ←↑↓→ move · Esc pause ",
            Style::default().fg(Color::DarkGray),
        )
    }
}

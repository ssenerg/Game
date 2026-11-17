use std::time::{Duration, Instant};

/// A key press, independent of where it came from: a local terminal or an
/// SSH connection.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Key {
    Up,
    Down,
    Left,
    Right,
    Enter,
    Esc,
    Backspace,
    Char(char),
    CtrlC,
    /// The left mouse button, at a cell of the screen. Only screens that
    /// ask for the mouse get these: see [`crate::GameUi::wants_mouse`].
    Mouse(Mouse),
    /// A character's key let go, on terminals that say so. Only screens
    /// that ask get these: see [`crate::GameUi::wants_key_releases`].
    Released(char),
}

/// How soon the same letter again counts as its key held down rather than
/// pressed again: past the first repeat at quick key-repeat settings, and
/// sooner than anyone presses again on purpose while aiming.
pub const REPEAT_WINDOW: Duration = Duration::from_millis(300);

/// On a terminal that can't say when a key is let go, a key held down just
/// repeats, and each repeat looks like another press: holding `f`, or a
/// press a little long at quick key-repeat settings, would throw at once.
/// Where a screen wants to hear keys let go, a letter coming again within
/// [`REPEAT_WINDOW`] of the last is taken for a repeat, so holding it is
/// one press, and pressing it again means it.
#[derive(Clone, Debug, Default)]
pub struct Repeats {
    last: Option<(char, Instant)>,
}

impl Repeats {
    /// Whether `key`, come at `now`, is the last letter's key still held.
    pub fn repeated(&mut self, key: Key, now: Instant) -> bool {
        let Key::Char(c) = key.lower() else {
            // Another key stops a held letter repeating.
            self.last = None;
            return false;
        };
        let again = self.last.is_some_and(|(last, at)| {
            last == c && now.saturating_duration_since(at) < REPEAT_WINDOW
        });
        // A repeat keeps the window open: the key is still down.
        self.last = Some((c, now));
        again
    }
}

/// The left button went down, moved while down, or came up at column `x`,
/// row `y`, from the top left.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Mouse {
    pub kind: MouseKind,
    pub x: u16,
    pub y: u16,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MouseKind {
    Press,
    /// The pointer moved to another cell with the button down.
    Drag,
    Release,
}

impl Mouse {
    /// Whether it's within `area`.
    pub fn within(self, area: ratatui::layout::Rect) -> bool {
        area.contains(ratatui::layout::Position::new(self.x, self.y))
    }
}

impl Key {
    /// Whether it's the mouse moving with its button down.
    pub fn is_drag(self) -> bool {
        matches!(
            self,
            Key::Mouse(Mouse {
                kind: MouseKind::Drag,
                ..
            })
        )
    }

    /// Moves `selected` up or down a list of `len` rows, wrapping around;
    /// `None` if this key doesn't move it.
    pub fn step(self, selected: usize, len: usize) -> Option<usize> {
        match self {
            Key::Up | Key::Char('w' | 'k') => Some(selected.checked_sub(1).unwrap_or(len - 1)),
            Key::Down | Key::Char('s' | 'j') => Some((selected + 1) % len),
            _ => None,
        }
    }

    /// Letters as lower case: Shift and Caps Lock don't change what a
    /// letter key does, except while typing.
    pub fn lower(self) -> Key {
        match self {
            Key::Char(c) => Key::Char(c.to_ascii_lowercase()),
            Key::Released(c) => Key::Released(c.to_ascii_lowercase()),
            key => key,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_letter_held_down_is_one_press_and_pressing_it_again_counts() {
        let start = Instant::now();
        let at = |ms| start + Duration::from_millis(ms);
        let mut repeats = Repeats::default();
        // Pressed, then repeating at a quick key-repeat setting.
        assert!(!repeats.repeated(Key::Char('f'), at(0)));
        for ms in [180, 210, 240, 270, 300, 330] {
            assert!(repeats.repeated(Key::Char('f'), at(ms)), "{ms} ms");
        }
        // Let go, and pressed again on purpose.
        assert!(!repeats.repeated(Key::Char('f'), at(1500)));
        assert!(repeats.repeated(Key::Char('F'), at(1600)), "case aside");
        // Another key between two presses: the letter is pressed anew.
        assert!(!repeats.repeated(Key::Up, at(1650)));
        assert!(!repeats.repeated(Key::Char('f'), at(1700)));
        assert!(
            !repeats.repeated(Key::Char('p'), at(1750)),
            "another letter"
        );
        assert!(!repeats.repeated(Key::Char('f'), at(1800)));
    }
}

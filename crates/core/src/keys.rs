//! Toolkit-neutral key events.
//!
//! Front ends translate their own events into these: the terminal client converts crossterm
//! events (with the `crossterm` feature), a GUI would convert its toolkit's. The names mirror
//! crossterm's so application code reads the same either way.
use std::ops::BitOr;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum KeyCode {
    Char(char),
    Enter,
    Esc,
    Backspace,
    Delete,
    Tab,
    BackTab,
    Up,
    Down,
    Left,
    Right,
    Home,
    End,
    PageUp,
    PageDown,
    Insert,
    F(u8),
    /// A key the application has no binding for.
    Other,
}

/// A set of held modifier keys.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct KeyModifiers(u8);

impl KeyModifiers {
    pub const NONE: Self = Self(0);
    pub const SHIFT: Self = Self(1);
    pub const CONTROL: Self = Self(1 << 1);
    pub const ALT: Self = Self(1 << 2);

    pub fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }

    /// Whether any of `other`'s modifiers is held.
    pub fn intersects(self, other: Self) -> bool {
        self.0 & other.0 != 0
    }

    pub fn is_empty(self) -> bool {
        self.0 == 0
    }
}

impl BitOr for KeyModifiers {
    type Output = Self;
    fn bitor(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum KeyEventKind {
    Press,
    Repeat,
    Release,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct KeyEvent {
    pub code: KeyCode,
    pub modifiers: KeyModifiers,
    pub kind: KeyEventKind,
}

impl KeyEvent {
    /// A key press, the only kind most front ends report.
    pub const fn new(code: KeyCode, modifiers: KeyModifiers) -> Self {
        Self {
            code,
            modifiers,
            kind: KeyEventKind::Press,
        }
    }

    pub const fn new_with_kind(code: KeyCode, modifiers: KeyModifiers, kind: KeyEventKind) -> Self {
        Self {
            code,
            modifiers,
            kind,
        }
    }
}

#[cfg(feature = "crossterm")]
mod from_crossterm {
    use super::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
    use crossterm::event as ct;

    impl From<ct::KeyCode> for KeyCode {
        fn from(code: ct::KeyCode) -> Self {
            match code {
                ct::KeyCode::Char(c) => Self::Char(c),
                ct::KeyCode::Enter => Self::Enter,
                ct::KeyCode::Esc => Self::Esc,
                ct::KeyCode::Backspace => Self::Backspace,
                ct::KeyCode::Delete => Self::Delete,
                ct::KeyCode::Tab => Self::Tab,
                ct::KeyCode::BackTab => Self::BackTab,
                ct::KeyCode::Up => Self::Up,
                ct::KeyCode::Down => Self::Down,
                ct::KeyCode::Left => Self::Left,
                ct::KeyCode::Right => Self::Right,
                ct::KeyCode::Home => Self::Home,
                ct::KeyCode::End => Self::End,
                ct::KeyCode::PageUp => Self::PageUp,
                ct::KeyCode::PageDown => Self::PageDown,
                ct::KeyCode::Insert => Self::Insert,
                ct::KeyCode::F(n) => Self::F(n),
                _ => Self::Other,
            }
        }
    }

    impl From<ct::KeyModifiers> for KeyModifiers {
        fn from(modifiers: ct::KeyModifiers) -> Self {
            [
                (ct::KeyModifiers::SHIFT, Self::SHIFT),
                (ct::KeyModifiers::CONTROL, Self::CONTROL),
                (ct::KeyModifiers::ALT, Self::ALT),
            ]
            .into_iter()
            .filter(|(held, _)| modifiers.contains(*held))
            .fold(Self::NONE, |set, (_, ours)| set | ours)
        }
    }

    impl From<ct::KeyEvent> for KeyEvent {
        fn from(key: ct::KeyEvent) -> Self {
            Self {
                code: key.code.into(),
                modifiers: key.modifiers.into(),
                kind: match key.kind {
                    ct::KeyEventKind::Press => KeyEventKind::Press,
                    ct::KeyEventKind::Repeat => KeyEventKind::Repeat,
                    ct::KeyEventKind::Release => KeyEventKind::Release,
                },
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn modifier_sets_combine_and_test_membership() {
        let held = KeyModifiers::CONTROL | KeyModifiers::SHIFT;
        assert!(held.contains(KeyModifiers::CONTROL) && held.contains(KeyModifiers::SHIFT));
        assert!(!held.contains(KeyModifiers::ALT));
        assert!(held.intersects(KeyModifiers::ALT | KeyModifiers::CONTROL));
        assert!(!held.intersects(KeyModifiers::ALT));
        assert!(KeyModifiers::NONE.is_empty() && !held.is_empty());
        assert_eq!(
            KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE).kind,
            KeyEventKind::Press
        );
    }
}

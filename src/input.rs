use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

use crate::app::Action;

pub fn action_for_key(key: KeyEvent) -> Option<Action> {
    if key.kind != KeyEventKind::Press {
        return None;
    }
    if key.modifiers == KeyModifiers::CONTROL && key.code == KeyCode::Char('c') {
        return Some(Action::Quit);
    }
    // Ignore unrelated shortcuts rather than treating e.g. Ctrl-P as a pull command.
    if key
        .modifiers
        .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT)
    {
        return None;
    }
    Some(match key.code {
        KeyCode::Left => Action::ScrollLeft,
        KeyCode::Right => Action::ScrollRight,
        KeyCode::PageDown => Action::PageDown,
        KeyCode::PageUp => Action::PageUp,
        KeyCode::Char('d') => Action::ToggleComparison,
        KeyCode::Char('v') => Action::ToggleSeen,
        KeyCode::Char('/') => Action::Search,
        KeyCode::Char('n') => Action::NextMatch,
        KeyCode::Char('q') => Action::Quit,
        KeyCode::Char('r') => Action::Refresh,
        KeyCode::Tab => Action::NextView,
        KeyCode::Char('j') | KeyCode::Down => Action::NextItem,
        KeyCode::Char('k') | KeyCode::Up => Action::PreviousItem,
        KeyCode::Enter => Action::SwitchBranch,
        KeyCode::Char('s') => Action::Stage,
        KeyCode::Char('u') => Action::Unstage,
        KeyCode::Char('c') => Action::Commit,
        KeyCode::Char('b') => Action::Branch,
        KeyCode::Char('f') => Action::Fetch,
        KeyCode::Char('p') => Action::Pull,
        KeyCode::Char('P') => Action::Push,
        KeyCode::Char('o') => Action::TakeOurs,
        KeyCode::Char('t') => Action::TakeTheirs,
        KeyCode::Char('a') => Action::MarkResolved,
        KeyCode::Char('e') => Action::Continue,
        KeyCode::Char('K') => Action::SkipRebase,
        KeyCode::Char('x') => Action::Abort,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn k_is_always_navigation_and_uppercase_k_is_explicit_skip() {
        assert_eq!(
            action_for_key(KeyEvent::new(KeyCode::Char('k'), KeyModifiers::NONE)),
            Some(Action::PreviousItem)
        );
        assert_eq!(
            action_for_key(KeyEvent::new(KeyCode::Char('K'), KeyModifiers::SHIFT)),
            Some(Action::SkipRebase)
        );
        assert_eq!(
            action_for_key(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE)),
            Some(Action::PreviousItem)
        );
    }

    #[test]
    fn control_c_quits_instead_of_committing_and_other_shortcuts_are_ignored() {
        assert_eq!(
            action_for_key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL)),
            Some(Action::Quit)
        );
        assert_eq!(
            action_for_key(KeyEvent::new(KeyCode::Char('p'), KeyModifiers::CONTROL)),
            None
        );
        assert_eq!(
            action_for_key(KeyEvent::new(KeyCode::Char('P'), KeyModifiers::ALT)),
            None
        );
    }

    #[test]
    fn key_releases_and_repeats_do_not_repeat_git_operations() {
        for kind in [KeyEventKind::Release, KeyEventKind::Repeat] {
            let key = KeyEvent::new_with_kind(KeyCode::Char('c'), KeyModifiers::NONE, kind);
            assert_eq!(action_for_key(key), None);
        }
    }
}

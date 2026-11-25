//! The state of a confirmation dialog in [`Mode::Confirm`]: the question it asks and what
//! confirming does.
//!
//! Every destructive operation that cannot be undone asks first, and so does deleting several
//! tasks at once; confirming performs the [`ConfirmKind`].

use std::mem;

use crate::app::effect::Effect;
use crate::app::lists::EntryKey;
use crate::app::state::{App, Mode};
use crate::markdown::document::TaskId;
use crate::storage::workspace::ListPath;

/// What a confirmation dialog asks for, with everything confirming it needs to know.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ConfirmKind {
    /// Move these list files and folders to the bin.
    DeleteEntries(Vec<EntryKey>),
    /// Delete the tasks `tasks` of the list at `list` with their subtasks.
    DeleteTasks {
        /// The list that holds the tasks.
        list: ListPath,
        /// The selected tasks, in document order.
        tasks: Vec<TaskId>,
    },
}

/// An open confirmation dialog: what it asks for and the question shown to the user.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ConfirmOverlay {
    kind: ConfirmKind,
    message: String,
}

impl ConfirmOverlay {
    /// Returns what the dialog asks for.
    #[must_use]
    pub fn kind(&self) -> &ConfirmKind {
        &self.kind
    }

    /// Returns the question shown to the user, such as `Delete empty folder "Work"?`.
    #[must_use]
    pub fn message(&self) -> &str {
        &self.message
    }
}

impl App {
    /// Asks the user to confirm `kind` with the question `message` and discards a pending
    /// key sequence.
    pub(super) fn open_confirm(&mut self, kind: ConfirmKind, message: String) {
        self.resolver.reset();
        self.mode = Mode::Confirm(ConfirmOverlay { kind, message });
    }

    /// Closes the open dialog, performs what it asked to confirm and returns the effects.
    pub(super) fn confirm(&mut self) -> Vec<Effect> {
        match mem::take(&mut self.mode) {
            Mode::Confirm(overlay) => {
                self.resolver.reset();
                match overlay.kind {
                    ConfirmKind::DeleteEntries(entries) => vec![Effect::DeleteEntries(entries)],
                    ConfirmKind::DeleteTasks { list, tasks } => self.delete_tasks(&list, &tasks),
                }
            }
            mode @ (Mode::Normal
            | Mode::Help(_)
            | Mode::Prompt(_)
            | Mode::InlineEdit(_)
            | Mode::Search(_)
            | Mode::Visual) => {
                self.mode = mode;
                Vec::new()
            }
        }
    }

    /// Closes the open dialog without effect and discards a pending key sequence.
    pub(super) fn close_confirm(&mut self) {
        match self.mode {
            Mode::Confirm(_) => {
                self.resolver.reset();
                self.mode = Mode::Normal;
            }
            Mode::Normal
            | Mode::Help(_)
            | Mode::Prompt(_)
            | Mode::InlineEdit(_)
            | Mode::Search(_)
            | Mode::Visual => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;
    use tempfile::TempDir;

    use std::path::PathBuf;

    use super::*;
    use crate::app::state::tests::app_in;
    use crate::config::keymap::action::Action;

    fn delete_work() -> ConfirmKind {
        ConfirmKind::DeleteEntries(vec![EntryKey::Folder(PathBuf::from("Work"))])
    }

    #[test]
    fn open_shows_the_question() {
        let directory = TempDir::new().unwrap();
        let mut app = app_in(directory.path());
        app.open_confirm(delete_work(), "Delete empty folder \"Work\"?".to_owned());
        let expected = ConfirmOverlay {
            kind: delete_work(),
            message: "Delete empty folder \"Work\"?".to_owned(),
        };

        let system_under_test = app.mode();

        assert!(matches!(system_under_test, Mode::Confirm(overlay) if *overlay == expected));
    }

    #[test]
    fn yes_closes_the_dialog_with_the_effect() {
        let directory = TempDir::new().unwrap();
        let mut app = app_in(directory.path());
        app.open_confirm(delete_work(), String::new());
        let expected = Effect::DeleteEntries(vec![EntryKey::Folder(PathBuf::from("Work"))]);

        let system_under_test = app.update(Action::ConfirmYes);

        assert_eq!(system_under_test, [expected]);
        assert!(matches!(app.mode(), Mode::Normal));
    }

    #[test]
    fn no_closes_the_dialog_without_effect() {
        let directory = TempDir::new().unwrap();
        let mut app = app_in(directory.path());
        app.open_confirm(delete_work(), String::new());

        let system_under_test = app.update(Action::ConfirmNo);

        assert_eq!(system_under_test, []);
        assert!(matches!(app.mode(), Mode::Normal));
    }

    #[rstest]
    #[case::yes(Action::ConfirmYes)]
    #[case::no(Action::ConfirmNo)]
    fn answers_without_a_dialog_keep_the_mode(#[case] action: Action) {
        let directory = TempDir::new().unwrap();
        let mut app = app_in(directory.path());
        app.open_help();

        let system_under_test = app.update(action);

        assert_eq!(system_under_test, []);
        assert!(matches!(app.mode(), Mode::Help(_)));
    }
}

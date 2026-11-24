//! The state of the inline task editor in [`Mode::InlineEdit`]: which task the typed text is
//! for, the [`TextInput`] that edits it in a row of the tasks pane, and what submitting it
//! does.
//!
//! The document is not touched while the user types. Submitting applies the whole change with
//! one [`App::edit_list`] call, so it is saved once and undone in one step, and cancelling
//! leaves the file as it was.

use crate::app::effect::Effect;
use crate::app::state::{App, Mode};
use crate::config::IndentStyle;
use crate::config::keymap::action::Action;
use crate::config::keymap::key::KeyChord;
use crate::markdown::document::{EditOutcome, InsertPosition, TaskId, TodoDocument};
use crate::storage::workspace::ListPath;
use crate::ui::outline::Outline;
use crate::ui::text_input::{TextChange, TextInput};

/// Which task the text of the inline editor is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum InlineEditKind {
    /// A new task, inserted at the position when the text is submitted; until then the tasks
    /// pane shows a placeholder row there.
    New(InsertPosition),
}

impl InlineEditKind {
    /// Applies `text` to `document` and returns the id of the task it went into, or `None` if
    /// the document was not changed.
    fn apply(
        self,
        document: &mut TodoDocument,
        text: &str,
        fallback_indent: IndentStyle,
    ) -> Option<TaskId> {
        match self {
            Self::New(position) => document.insert_task(position, text, fallback_indent),
        }
    }
}

/// An open inline editor: the list it edits, which task the text is for and the typed text.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct InlineEdit {
    list: ListPath,
    kind: InlineEditKind,
    input: TextInput,
}

impl InlineEdit {
    /// Returns which task the typed text is for.
    #[must_use]
    pub fn kind(&self) -> InlineEditKind {
        self.kind
    }

    /// Returns the input with the typed text, as the tasks pane draws it.
    #[must_use]
    pub fn input(&self) -> &TextInput {
        &self.input
    }

    /// Edits the text with a key press that no action is bound to.
    pub(super) fn press(&mut self, chord: KeyChord) {
        // The row shows the text whether or not it changed.
        let _: TextChange = self.input.press(chord);
    }

    /// Inserts pasted text at the cursor.
    pub(super) fn paste(&mut self, text: &str) {
        self.input.insert_str(text);
    }
}

impl App {
    /// Opens the inline editor for a new task at `position` relative to the selected task,
    /// or at the end of a list without tasks.
    ///
    /// Nothing happens without an open list or if it could not be loaded.
    pub(super) fn add_task(&mut self, position: fn(TaskId) -> InsertPosition) {
        let Some(list) = self.active_list().filter(|list| list.content().is_ok()) else {
            return;
        };
        let list = list.path().clone();
        let position = self
            .task_outline()
            .and_then(Outline::selected)
            .map_or(InsertPosition::End, |&selected| position(selected));
        self.resolver.reset();
        self.mode = Mode::InlineEdit(InlineEdit {
            list,
            kind: InlineEditKind::New(position),
            input: TextInput::new(),
        });
    }

    /// Unfolds the selected task and opens the inline editor for its new last subtask.
    pub(super) fn add_subtask(&mut self) {
        self.update_tasks(Action::Expand);
        self.add_task(InsertPosition::LastChild);
    }

    /// Closes the inline editor and applies its text with one [`App::edit_list`] call, then
    /// selects the task it went into. Empty text changes nothing.
    pub(super) fn submit_inline_edit(&mut self) -> Vec<Effect> {
        let Some(edit) = self.close_inline_edit() else {
            return Vec::new();
        };
        let text = edit.input.value();
        if text.trim().is_empty() {
            return Vec::new();
        }
        let fallback_indent = self.config.tasks().indent();
        let mut edited = None;
        let effects = self.edit_list(&edit.list, |document| {
            edited = edit.kind.apply(document, text, fallback_indent);
            match edited {
                Some(_) => EditOutcome::Changed,
                None => EditOutcome::Unchanged,
            }
        });
        if let Some(id) = edited {
            self.select_task(id);
        }
        effects
    }

    /// Closes the inline editor without changing the document.
    pub(super) fn cancel_inline_edit(&mut self) {
        let _: Option<InlineEdit> = self.close_inline_edit();
    }

    /// Leaves the inline editor, discards a pending key sequence and returns the editor, or
    /// keeps the mode and returns `None` if no inline editor is open.
    fn close_inline_edit(&mut self) -> Option<InlineEdit> {
        match std::mem::take(&mut self.mode) {
            Mode::InlineEdit(edit) => {
                self.resolver.reset();
                Some(edit)
            }
            mode @ (Mode::Normal | Mode::Help(_) | Mode::Prompt(_) | Mode::Confirm(_)) => {
                self.mode = mode;
                None
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::TempDir;

    use super::*;
    use crate::app::state::Focus;
    use crate::app::state::tests::{app_in, only_list_path};

    fn inline_edit_of(app: &App) -> &InlineEdit {
        match app.mode() {
            Mode::InlineEdit(edit) => edit,
            mode => panic!("no inline editor is open, the mode is {mode:?}"),
        }
    }

    /// Starts an app on a workspace with the single list `content`, with the tasks pane
    /// focused.
    fn app_with_list(directory: &TempDir, content: &str) -> App {
        fs::write(directory.path().join("plan.md"), content).unwrap();
        let mut app = app_in(directory.path());
        app.focus = Focus::Tasks;
        app
    }

    #[test]
    fn submit_saves_once() {
        let directory = TempDir::new().unwrap();
        let mut app = app_with_list(&directory, "- [ ] a\n");
        let list = only_list_path(&app);
        app.add_task(InsertPosition::Below);
        let _effects = app.handle_paste("b");

        let system_under_test = app.submit_inline_edit();

        assert_eq!(system_under_test, [Effect::Save(list)]);
    }

    #[test]
    fn add_task_relates_the_position_to_the_selected_task() {
        let directory = TempDir::new().unwrap();
        let mut app = app_with_list(&directory, "- [ ] a\n");
        let selected = *app.task_outline().unwrap().selected().unwrap();
        app.add_task(InsertPosition::Above);

        let system_under_test = inline_edit_of(&app);

        assert_eq!(
            system_under_test.kind(),
            InlineEditKind::New(InsertPosition::Above(selected))
        );
        assert_eq!(system_under_test.input().value(), "");
    }

    #[test]
    fn add_task_without_an_open_list_keeps_the_mode() {
        let directory = TempDir::new().unwrap();
        let mut app = app_in(directory.path());
        app.add_task(InsertPosition::Below);

        let system_under_test = app.mode();

        assert!(matches!(system_under_test, Mode::Normal));
    }

    #[test]
    fn add_task_in_a_list_that_could_not_be_loaded_keeps_the_mode() {
        let directory = TempDir::new().unwrap();
        fs::write(directory.path().join("binary.md"), [0xff, 0xfe]).unwrap();
        let mut app = app_in(directory.path());
        app.add_task(InsertPosition::Below);

        let system_under_test = app.mode();

        assert!(matches!(system_under_test, Mode::Normal));
    }

    #[test]
    fn insert_next_to_a_task_removed_meanwhile_changes_nothing() {
        let directory = TempDir::new().unwrap();
        let mut app = app_with_list(&directory, "- [ ] a\n- [ ] b\n");
        let list = only_list_path(&app);
        let removed = *app.task_outline().unwrap().selected().unwrap();
        app.add_task(InsertPosition::Below);
        let _effects = app.handle_paste("c");
        let _effects = app.edit_list(&list, |document| match document.remove_block(removed) {
            Some(_) => EditOutcome::Changed,
            None => EditOutcome::Unchanged,
        });

        let system_under_test = app.submit_inline_edit();

        assert_eq!(system_under_test, []);
        assert!(matches!(app.mode(), Mode::Normal));
    }

    #[test]
    fn closing_without_an_inline_editor_keeps_the_mode() {
        let directory = TempDir::new().unwrap();
        let mut app = app_in(directory.path());
        app.open_help();
        app.cancel_inline_edit();

        let system_under_test = app.mode();

        assert!(matches!(system_under_test, Mode::Help(_)));
    }

    #[test]
    fn submit_without_an_inline_editor_has_no_effect() {
        let directory = TempDir::new().unwrap();
        let mut app = app_in(directory.path());

        let system_under_test = app.submit_inline_edit();

        assert_eq!(system_under_test, []);
    }
}

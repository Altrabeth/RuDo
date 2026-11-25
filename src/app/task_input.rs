//! The state of the inline task editor in [`Mode::InlineEdit`]: which task the typed text is
//! for, a new or an existing one, the [`TextInput`] that edits it in a row of the tasks pane,
//! and what submitting it does.
//!
//! The document is not touched while the user types. Submitting applies the whole change with
//! one [`App::edit_list`] call, so it is saved once and undone in one step, and cancelling
//! leaves the file as it was.

use jiff::civil::Date;

use crate::app::effect::Effect;
use crate::app::state::{App, Mode};
use crate::app::status::StatusMessage;
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
    /// An existing task, whose description the text replaces; its status, inline fields and
    /// subtasks are kept.
    Existing(TaskId),
}

/// Shown if the edited task was removed meanwhile, for example by a reload.
pub(super) const TASK_GONE: &str = "the edited task no longer exists";

/// Shown if the list of a new task was removed or cannot be loaded any more, for example
/// after a rescan.
const LIST_GONE: &str = "the edited list no longer exists";

/// What applying the text of the inline editor did to the document.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Applied {
    /// The document changed; the task with this id holds the text.
    Changed(TaskId),
    /// The document already held the text, or a new task had nowhere to go.
    Unchanged,
    /// The edited task is not in the document any more.
    TaskGone,
    /// The edited list is not in the workspace any more or cannot be loaded.
    ListGone,
}

impl InlineEditKind {
    /// Applies `text` to `document` and returns what it did; a new task gets the creation
    /// date `created`.
    fn apply(
        self,
        document: &mut TodoDocument,
        text: &str,
        created: Option<Date>,
        fallback_indent: IndentStyle,
    ) -> Applied {
        match self {
            Self::New(position) => document
                .insert_task(position, text, created, fallback_indent)
                .map_or(Applied::Unchanged, Applied::Changed),
            // `set_description` reports a missing task as unchanged, so look it up first.
            Self::Existing(id) if document.depth(id).is_none() => Applied::TaskGone,
            Self::Existing(id) => match document.set_description(id, text) {
                EditOutcome::Changed => Applied::Changed(id),
                EditOutcome::Unchanged => Applied::Unchanged,
            },
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
    /// Returns the creation date of a task created now: today, or `None` if
    /// `tasks.created_date` is off.
    pub(super) fn created_date(&self) -> Option<Date> {
        self.config.tasks().created_date().then_some(self.today)
    }

    /// Opens the inline editor for a new task at `position` relative to the selected task,
    /// or at the end of a list without tasks.
    ///
    /// Nothing happens without an open list or if it could not be loaded.
    pub(super) fn add_task(&mut self, position: fn(TaskId) -> InsertPosition) {
        let Some(list) = self.active_list().filter(|list| list.content().is_ok()) else {
            return;
        };
        let list = list.path().clone();
        let position = self.insert_position(position);
        self.open_inline_edit(InlineEdit {
            list,
            kind: InlineEditKind::New(position),
            input: TextInput::new(),
        });
    }

    /// Opens the inline editor for the selected task, prefilled with its description without
    /// inline fields and with the cursor at the end.
    ///
    /// Nothing happens without a selected task.
    pub(super) fn edit_task(&mut self) {
        let Some(&id) = self.task_outline().and_then(Outline::selected) else {
            return;
        };
        let Some(list) = self.active_list() else {
            return;
        };
        // A list that could not be loaded has no tasks to select; its pane shows the error.
        let Ok(loaded) = list.content() else {
            return;
        };
        let Some(node) = loaded.document().tasks().find(|node| node.id() == id) else {
            return;
        };
        let edit = InlineEdit {
            list: list.path().clone(),
            kind: InlineEditKind::Existing(id),
            input: TextInput::with_value(node.task().description()),
        };
        self.open_inline_edit(edit);
    }

    /// Enters [`Mode::InlineEdit`] with `edit` and discards a pending key sequence.
    fn open_inline_edit(&mut self, edit: InlineEdit) {
        self.resolver.reset();
        self.mode = Mode::InlineEdit(edit);
    }

    /// Unfolds the selected task and opens the inline editor for its new last subtask.
    pub(super) fn add_subtask(&mut self) {
        self.update_tasks(Action::Expand);
        self.add_task(InsertPosition::LastChild);
    }

    /// Closes the inline editor and applies its text with one [`App::edit_list`] call, then
    /// selects the task it went into. Empty text changes nothing, and an error message is
    /// shown if the edited task or its list was removed meanwhile.
    pub(super) fn submit_inline_edit(&mut self) -> Vec<Effect> {
        let Some(edit) = self.close_inline_edit() else {
            return Vec::new();
        };
        let text = edit.input.value();
        if text.trim().is_empty() {
            return Vec::new();
        }
        let created = self.created_date();
        let fallback_indent = self.config.tasks().indent();
        // `edit_list` only calls the edit for a loaded list.
        let mut applied = Applied::ListGone;
        let effects = self.edit_list(&edit.list, |document| {
            applied = edit.kind.apply(document, text, created, fallback_indent);
            match applied {
                Applied::Changed(_) => EditOutcome::Changed,
                Applied::Unchanged | Applied::TaskGone | Applied::ListGone => {
                    EditOutcome::Unchanged
                }
            }
        });
        match applied {
            Applied::Changed(id) => self.select_task(id),
            Applied::Unchanged => {}
            Applied::TaskGone => self.show_message(StatusMessage::error(TASK_GONE)),
            Applied::ListGone => self.show_message(StatusMessage::error(LIST_GONE)),
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
            mode @ (Mode::Normal
            | Mode::Help(_)
            | Mode::Prompt(_)
            | Mode::FolderPicker(_)
            | Mode::GoTo(_)
            | Mode::Confirm(_)
            | Mode::Search(_)
            | Mode::Visual) => {
                self.mode = mode;
                None
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use rstest::rstest;
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
    fn edit_task_prefills_the_selected_task() {
        let directory = TempDir::new().unwrap();
        let mut app = app_with_list(&directory, "- [ ] a #b [due:: 2026-12-01]\n");
        let selected = *app.task_outline().unwrap().selected().unwrap();
        let expected_text = "a #b";
        app.edit_task();

        let system_under_test = inline_edit_of(&app);

        assert_eq!(system_under_test.kind(), InlineEditKind::Existing(selected));
        assert_eq!(system_under_test.input().value(), expected_text);
        assert_eq!(system_under_test.input().cursor(), expected_text.len());
    }

    #[test]
    fn edit_task_without_tasks_keeps_the_mode() {
        let directory = TempDir::new().unwrap();
        let mut app = app_with_list(&directory, "# Notes\n");
        app.edit_task();

        let system_under_test = app.mode();

        assert!(matches!(system_under_test, Mode::Normal));
    }

    #[rstest]
    #[case::changed("c", true)]
    #[case::unchanged("a", false)]
    fn apply_to_an_existing_task_sets_its_description(#[case] text: &str, #[case] changes: bool) {
        let mut document = TodoDocument::parse("- [ ] a\n");
        let id = document.tasks().next().unwrap().id();
        let expected = if changes {
            Applied::Changed(id)
        } else {
            Applied::Unchanged
        };

        let system_under_test =
            InlineEditKind::Existing(id).apply(&mut document, text, None, IndentStyle::Tab);

        assert_eq!(system_under_test, expected);
    }

    #[test]
    fn apply_to_a_task_that_is_gone_reports_it() {
        let mut document = TodoDocument::parse("- [ ] a\n");
        let id = document.tasks().next().unwrap().id();
        let _removed = document.remove_block(id);

        let system_under_test =
            InlineEditKind::Existing(id).apply(&mut document, "b", None, IndentStyle::Tab);

        assert_eq!(system_under_test, Applied::TaskGone);
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

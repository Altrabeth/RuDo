//! The decisions of the application: key presses and actions change the [`App`] and return
//! the [`Effect`]s to perform.

use crate::app::effect::Effect;
use crate::app::prompt::PromptKind;
use crate::app::state::{App, Focus, Mode};
use crate::config::keymap::action::Action;
use crate::config::keymap::key::KeyChord;
use crate::config::keymap::resolver::Resolution;
use crate::markdown::document::{EditOutcome, InsertPosition, TodoDocument};
use crate::storage::workspace::ListPath;
use crate::ui::outline::OutlineCommand;
use crate::ui::text_input::TextInput;

impl App {
    /// Handles a key press: resolves `chord` in the current [`App::context`] and performs the
    /// action it completes.
    ///
    /// A chord that only begins a key sequence is remembered until the sequence is complete.
    /// Any key press hides an error message.
    #[must_use]
    pub fn handle_key(&mut self, chord: KeyChord) -> Vec<Effect> {
        self.status.key_pressed();
        match self.resolver.feed(&self.keymap, self.context(), chord) {
            Resolution::Action(action) => self.update(action),
            Resolution::NoMatch => {
                self.type_key(chord);
                Vec::new()
            }
            Resolution::Pending | Resolution::Cancelled => Vec::new(),
        }
    }

    /// Handles text pasted into the terminal.
    ///
    /// Pasted text is never interpreted as key presses: an open text input inserts it, and
    /// otherwise it is ignored, so that pasting by mistake cannot trigger a burst of commands.
    #[must_use]
    pub fn handle_paste(&mut self, text: &str) -> Vec<Effect> {
        match &mut self.mode {
            Mode::Normal | Mode::Confirm(_) => {
                tracing::debug!(
                    length = text.len(),
                    "pasted text ignored without a text input"
                );
            }
            Mode::Help(help) => help.paste(text, &self.keymap, self.screen_size),
            Mode::Prompt(prompt) => prompt.paste(text),
            Mode::InlineEdit(edit) => edit.paste(text),
        }
        Vec::new()
    }

    /// Performs `action` and returns the effects it requires.
    ///
    /// Actions that have no effect yet are accepted and ignored.
    #[must_use]
    pub fn update(&mut self, action: Action) -> Vec<Effect> {
        match action {
            Action::Quit => self.should_quit = true,
            Action::FocusNext => self.focus = self.focus.next(),
            Action::FocusPrevious => self.focus = self.focus.previous(),
            Action::FocusLists => self.focus = Focus::Lists,
            Action::FocusTasks => self.focus = Focus::Tasks,
            action @ (Action::MoveDown
            | Action::MoveUp
            | Action::MoveToTop
            | Action::MoveToBottom
            | Action::HalfPageDown
            | Action::HalfPageUp
            | Action::Collapse
            | Action::Expand
            | Action::ToggleFold
            | Action::FoldAll
            | Action::UnfoldAll
            | Action::Open) => self.navigate(action),
            Action::Search => self.search(),
            Action::Submit => return self.submit(),
            Action::Cancel => self.cancel(),
            Action::Help => self.open_help(),
            Action::CloseHelp => self.close_help(),
            Action::NewList => self.open_prompt(
                PromptKind::NewList {
                    parent: self.lists.target_folder(),
                },
                TextInput::new(),
            ),
            Action::NewFolder => self.open_prompt(
                PromptKind::NewFolder {
                    parent: self.lists.target_folder(),
                },
                TextInput::new(),
            ),
            Action::Rename => self.rename_selected_entry(),
            Action::Delete => match self.focus {
                Focus::Lists => self.delete_selected_entry(),
                Focus::Tasks => {}
            },
            Action::ConfirmYes => return self.confirm(),
            Action::ConfirmNo => self.close_confirm(),
            Action::Indent => return self.indent_task(),
            Action::MoveTaskDown => return self.move_task_down(),
            Action::MoveTaskUp => return self.move_task_up(),
            Action::Outdent => return self.outdent_task(),
            Action::ToggleDone => return self.toggle_done(),
            Action::NewTaskBelow => self.add_task(InsertPosition::Below),
            Action::NewTaskAbove => self.add_task(InsertPosition::Above),
            Action::NewSubtask => self.add_subtask(),
            Action::ClearFilter
            | Action::Undo
            | Action::Redo
            | Action::ToggleCompleted
            | Action::OpenInEditor
            | Action::Edit
            | Action::Yank
            | Action::PasteBelow
            | Action::PasteAbove
            | Action::PriorityUp
            | Action::PriorityDown
            | Action::SetDueDate
            | Action::SetRecurrence
            | Action::SortByPriority
            | Action::SortByDueDate
            | Action::SortAlphabetically
            | Action::SortByStatus => {}
        }
        Vec::new()
    }

    /// Routes a navigation or tree action to whatever takes the keys: the help screen if it is
    /// open, otherwise the focused pane.
    fn navigate(&mut self, action: Action) {
        match &mut self.mode {
            Mode::Help(help) => {
                if let Some(command) = OutlineCommand::from_action(action) {
                    help.navigate(command, self.screen_size);
                }
            }
            Mode::Normal => match self.focus {
                Focus::Lists => self.update_lists(action),
                Focus::Tasks => self.update_tasks(action),
            },
            Mode::Prompt(_) | Mode::Confirm(_) | Mode::InlineEdit(_) => {}
        }
    }

    /// Opens the filter of whatever takes the keys; so far only the help screen has one.
    fn search(&mut self) {
        match &mut self.mode {
            Mode::Help(help) => help.open_filter(self.screen_size),
            Mode::Normal | Mode::Prompt(_) | Mode::Confirm(_) | Mode::InlineEdit(_) => {}
        }
    }

    /// Accepts the text of the open text input and returns the effects it asks for.
    fn submit(&mut self) -> Vec<Effect> {
        match &mut self.mode {
            Mode::Help(help) => help.submit_filter(self.screen_size),
            Mode::Prompt(_) => return self.submit_prompt(),
            Mode::InlineEdit(_) => return self.submit_inline_edit(),
            Mode::Normal | Mode::Confirm(_) => {}
        }
        Vec::new()
    }

    /// Discards the text of the open text input.
    fn cancel(&mut self) {
        match &mut self.mode {
            Mode::Help(help) => help.cancel_filter(&self.keymap, self.screen_size),
            Mode::Prompt(_) => self.close_prompt(),
            Mode::InlineEdit(_) => self.cancel_inline_edit(),
            Mode::Normal | Mode::Confirm(_) => {}
        }
    }

    /// Hands a key press that no action is bound to to the open text input, if any.
    fn type_key(&mut self, chord: KeyChord) {
        match &mut self.mode {
            Mode::Help(help) => help.press(chord, &self.keymap, self.screen_size),
            Mode::Prompt(prompt) => prompt.press(chord),
            Mode::InlineEdit(edit) => edit.press(chord),
            Mode::Normal | Mode::Confirm(_) => {}
        }
    }

    /// Applies `edit` to the document of the list at `list` and returns [`Effect::Save`] for
    /// it if the edit changed the document.
    ///
    /// This is the only way the application changes a document, so saving (and undo) cannot
    /// be forgotten by a feature. A list that is not in the workspace or could not be loaded
    /// is not edited.
    #[must_use]
    pub fn edit_list(
        &mut self,
        list: &ListPath,
        edit: impl FnOnce(&mut TodoDocument) -> EditOutcome,
    ) -> Vec<Effect> {
        let Some(Ok(loaded)) = self
            .workspace
            .list_mut(list)
            .map(|found| found.content_mut())
        else {
            return Vec::new();
        };
        match edit(loaded.document_mut()) {
            EditOutcome::Changed => vec![Effect::Save(list.clone())],
            EditOutcome::Unchanged => Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use rstest::rstest;
    use strum::IntoEnumIterator;
    use tempfile::TempDir;

    use super::*;
    use crate::app::state::tests::{app_in, only_list_path, toggle_first_task};

    #[rstest]
    #[case::sequence_prefix("g")]
    #[case::unbound("f12")]
    fn keys_without_an_action_have_no_effect(#[case] notation: &str) {
        let directory = TempDir::new().unwrap();
        let mut app = app_in(directory.path());
        let chord: KeyChord = notation.parse().unwrap();

        let system_under_test = app.handle_key(chord);

        assert_eq!(system_under_test, []);
        assert_eq!(app.focus(), Focus::Lists);
        assert!(!app.should_quit());
    }

    #[test]
    fn bound_key_performs_its_action() {
        let directory = TempDir::new().unwrap();
        let mut app = app_in(directory.path());
        let chord: KeyChord = "tab".parse().unwrap();

        let system_under_test = app.handle_key(chord);

        assert_eq!(system_under_test, []);
        assert_eq!(app.focus(), Focus::Tasks);
    }

    #[test]
    fn paste_in_normal_mode_is_not_interpreted_as_keys() {
        let directory = TempDir::new().unwrap();
        let mut app = app_in(directory.path());
        let text = "q\t";

        let system_under_test = app.handle_paste(text);

        assert_eq!(system_under_test, []);
        assert_eq!(app.focus(), Focus::Lists);
        assert!(!app.should_quit());
    }

    #[test]
    fn no_action_returns_an_effect_yet() {
        let directory = TempDir::new().unwrap();
        let mut app = app_in(directory.path());

        let system_under_test: Vec<Effect> = Action::iter()
            .flat_map(|action| app.update(action))
            .collect();

        assert_eq!(system_under_test, []);
    }

    #[test]
    fn changing_edit_returns_a_save_and_changes_the_document_in_memory() {
        let directory = TempDir::new().unwrap();
        fs::write(directory.path().join("groceries.md"), "- [ ] Buy milk\n").unwrap();
        let mut app = app_in(directory.path());
        let list = only_list_path(&app);
        let expected_document = "- [x] Buy milk\n";

        let system_under_test = app.edit_list(&list, toggle_first_task);

        assert_eq!(system_under_test, [Effect::Save(list.clone())]);
        let loaded = app.workspace().list(&list).unwrap().content().unwrap();
        assert_eq!(loaded.document().to_string(), expected_document);
    }

    #[test]
    fn unchanging_edit_returns_no_effect() {
        let directory = TempDir::new().unwrap();
        fs::write(directory.path().join("groceries.md"), "- [ ] Buy milk\n").unwrap();
        let mut app = app_in(directory.path());
        let list = only_list_path(&app);

        let system_under_test = app.edit_list(&list, |_| EditOutcome::Unchanged);

        assert_eq!(system_under_test, []);
    }

    #[test]
    fn list_that_could_not_be_loaded_is_not_edited() {
        let directory = TempDir::new().unwrap();
        fs::write(directory.path().join("binary.md"), [0xff, 0xfe]).unwrap();
        let mut app = app_in(directory.path());
        let list = only_list_path(&app);

        let system_under_test = app.edit_list(&list, |_| EditOutcome::Changed);

        assert_eq!(system_under_test, []);
    }

    #[test]
    fn list_missing_from_the_workspace_is_not_edited() {
        let other_directory = TempDir::new().unwrap();
        fs::write(other_directory.path().join("other.md"), "").unwrap();
        let list = only_list_path(&app_in(other_directory.path()));
        let directory = TempDir::new().unwrap();
        let mut app = app_in(directory.path());

        let system_under_test = app.edit_list(&list, |_| EditOutcome::Changed);

        assert_eq!(system_under_test, []);
    }
}

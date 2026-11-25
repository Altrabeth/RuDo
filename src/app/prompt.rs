//! The state of an input prompt in [`Mode::Prompt`]: what the typed text is for, the
//! [`Prompt`] popup that edits it, and what submitting it does.
//!
//! The typed text is validated here, without touching the disk; a valid text becomes an
//! [`Effect`] whose result comes back to the [`App`].

use std::borrow::Cow;
use std::path::{Path, PathBuf};

use jiff::civil::Date;

use crate::app::effect::Effect;
use crate::app::lists::{EntryKey, check_move_target};
use crate::app::state::{App, Mode};
use crate::app::task_due::{DUE_DATE_HINT, due_preview, parse_due_text};
use crate::app::task_recurrence::{RECURRENCE_HINT, parse_recurrence_text, recurrence_preview};
use crate::config::keymap::key::KeyChord;
use crate::domain::recurrence::Recurrence;
use crate::markdown::document::TaskId;
use crate::storage::entries::EntryName;
use crate::storage::workspace::{EntryKind, ListPath};
use crate::ui::popup::Prompt;
use crate::ui::text_input::{TextChange, TextInput};

/// What the text typed into a prompt is for, with everything submitting it needs to know.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum PromptKind {
    /// The name of a new list in the folder `parent`, relative to the workspace root and
    /// empty for the root itself.
    NewList {
        /// The folder that receives the list.
        parent: PathBuf,
    },
    /// The name of a new folder in the folder `parent`, relative to the workspace root and
    /// empty for the root itself.
    NewFolder {
        /// The folder that receives the folder.
        parent: PathBuf,
    },
    /// The new name of the list file or folder at `path`, relative to the workspace root.
    Rename {
        /// Whether the entry is a list file or a folder.
        kind: EntryKind,
        /// The entry to rename.
        path: PathBuf,
    },
    /// The folder that receives the lists and folders `entries`, relative to the workspace
    /// root and empty for the root itself.
    MoveToFolder {
        /// The lists and folders to move.
        entries: Vec<EntryKey>,
    },
    /// The due date of the task `task` in the list at `list`, as a due date expression.
    DueDate {
        /// The list that holds the task.
        list: ListPath,
        /// The task whose due date is set or cleared.
        task: TaskId,
    },
    /// The recurrence rule of the task `task` in the list at `list`.
    Recurrence {
        /// The list that holds the task.
        list: ListPath,
        /// The task whose recurrence rule is set or cleared.
        task: TaskId,
    },
}

impl PromptKind {
    /// Returns the title of the popup.
    const fn title(&self) -> &'static str {
        match self {
            Self::NewList { .. } => "New list",
            Self::NewFolder { .. } => "New folder",
            Self::Rename {
                kind: EntryKind::List,
                ..
            } => "Rename list",
            Self::Rename {
                kind: EntryKind::Folder,
                ..
            } => "Rename folder",
            Self::MoveToFolder { .. } => "Move to folder",
            Self::DueDate { .. } => "Due date",
            Self::Recurrence { .. } => "Recurrence",
        }
    }

    /// Returns the hint below the input: the folder of the new or renamed entry, how to name
    /// the target folder of a move, or the accepted due date expressions or recurrence rules.
    fn hint(&self) -> Cow<'static, str> {
        match self {
            Self::NewList { parent } | Self::NewFolder { parent } => location_hint(parent),
            Self::Rename { path, .. } => location_hint(path.parent().unwrap_or(Path::new(""))),
            Self::MoveToFolder { .. } => Cow::Borrowed(MOVE_HINT),
            Self::DueDate { .. } => Cow::Borrowed(DUE_DATE_HINT),
            Self::Recurrence { .. } => Cow::Borrowed(RECURRENCE_HINT),
        }
    }

    /// Returns the preview of `text` relative to `today`, or why it cannot be submitted.
    ///
    /// Recomputed on every edit; prompts whose text is checked only on submit have no preview.
    fn preview(
        &self,
        text: &str,
        today: Date,
    ) -> Result<Option<Cow<'static, str>>, Cow<'static, str>> {
        match self {
            Self::NewList { .. }
            | Self::NewFolder { .. }
            | Self::Rename { .. }
            | Self::MoveToFolder { .. } => Ok(None),
            Self::DueDate { .. } => due_preview(text, today).map(Some),
            Self::Recurrence { .. } => recurrence_preview(text).map(Some),
        }
    }

    /// Parses `text` relative to `today` and returns what the prompt asks for.
    fn submit(&self, text: &str, today: Date) -> Result<Submission, Cow<'static, str>> {
        let name = || EntryName::try_from(text).map_err(|error| error.to_string());
        Ok(match self {
            Self::NewList { parent } => Submission::Effect(Effect::CreateEntry {
                kind: EntryKind::List,
                parent: parent.clone(),
                name: name()?,
            }),
            Self::NewFolder { parent } => Submission::Effect(Effect::CreateEntry {
                kind: EntryKind::Folder,
                parent: parent.clone(),
                name: name()?,
            }),
            Self::Rename { path, .. } => Submission::Effect(Effect::RenameEntry {
                path: path.clone(),
                name: name()?,
            }),
            Self::MoveToFolder { entries } => Submission::Effect(Effect::MoveEntries {
                entries: entries.clone(),
                folder: folder_path(text),
            }),
            Self::DueDate { list, task } => Submission::SetDue {
                list: list.clone(),
                task: *task,
                due: parse_due_text(text, today)?,
            },
            Self::Recurrence { list, task } => Submission::SetRecurrence {
                list: list.clone(),
                task: *task,
                recurrence: parse_recurrence_text(text)?,
            },
        })
    }
}

/// What submitting a valid text asks for.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Submission {
    /// An effect to perform.
    Effect(Effect),
    /// Setting the due date of `task` in the list at `list`, or clearing it if `due` is `None`.
    SetDue {
        list: ListPath,
        task: TaskId,
        due: Option<Date>,
    },
    /// Setting the recurrence rule of `task` in the list at `list`, or clearing it if
    /// `recurrence` is `None`.
    SetRecurrence {
        list: ListPath,
        task: TaskId,
        recurrence: Option<Recurrence>,
    },
}

/// Explains the text of the prompt for the target folder of a move.
const MOVE_HINT: &str = "like Home/Garden; empty for the top level";

/// Returns the folder named by `text`, relative to the workspace root: surrounding whitespace
/// and slashes are ignored, so that empty text names the root.
fn folder_path(text: &str) -> PathBuf {
    PathBuf::from(text.trim().trim_matches('/'))
}

/// Describes the folder `parent` of a new or renamed entry.
fn location_hint(parent: &Path) -> Cow<'static, str> {
    if parent.as_os_str().is_empty() {
        Cow::Borrowed("in the workspace folder")
    } else {
        Cow::Owned(format!("in {}", parent.display()))
    }
}

/// An open prompt: what it asks for and the popup with the typed text and its error.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct PromptOverlay {
    kind: PromptKind,
    prompt: Prompt,
}

impl PromptOverlay {
    /// Opens a prompt for `kind` that edits `input`, with its preview relative to `today`.
    fn open(kind: PromptKind, input: TextInput, today: Date) -> Self {
        let prompt = Prompt::new(kind.title(), input).with_hint(kind.hint());
        let mut overlay = Self { kind, prompt };
        overlay.refresh(today);
        overlay
    }

    /// Returns the popup with the typed text and the error, as the view draws it.
    #[must_use]
    pub fn prompt(&self) -> &Prompt {
        &self.prompt
    }

    /// Edits the text with a key press that no action is bound to; a change recomputes the
    /// preview relative to `today` and replaces the error.
    pub(super) fn press(&mut self, chord: KeyChord, today: Date) {
        match self.prompt.input_mut().press(chord) {
            TextChange::Changed => self.refresh(today),
            TextChange::Unchanged => {}
        }
    }

    /// Inserts pasted text and recomputes the preview relative to `today`.
    pub(super) fn paste(&mut self, text: &str, today: Date) {
        self.prompt.input_mut().insert_str(text);
        self.refresh(today);
    }

    /// Shows the preview of the current text relative to `today`, or why it is invalid.
    fn refresh(&mut self, today: Date) {
        let (preview, error) = match self.kind.preview(self.prompt.input().value(), today) {
            Ok(preview) => (preview, None),
            Err(error) => (None, Some(error)),
        };
        self.prompt.set_preview(preview);
        self.prompt.set_error(error);
    }

    /// Returns what the typed text asks for, or shows why the text is invalid and returns
    /// `None`.
    fn submit(&mut self, today: Date) -> Option<Submission> {
        match self.kind.submit(self.prompt.input().value(), today) {
            Ok(submission) => Some(submission),
            Err(error) => {
                self.prompt.set_error(Some(error));
                None
            }
        }
    }
}

impl App {
    /// Opens a prompt for `kind` that edits `input`, such as the current name of an entry to
    /// rename, and discards a pending key sequence.
    pub(super) fn open_prompt(&mut self, kind: PromptKind, input: TextInput) {
        self.resolver.reset();
        self.mode = Mode::Prompt(PromptOverlay::open(kind, input, self.today));
    }

    /// Closes the open prompt and returns the effects of its text; an invalid text keeps the
    /// prompt open and shows why.
    pub(super) fn submit_prompt(&mut self) -> Vec<Effect> {
        let Mode::Prompt(prompt) = &mut self.mode else {
            return Vec::new();
        };
        let Some(submission) = prompt.submit(self.today) else {
            return Vec::new();
        };
        if let Submission::Effect(Effect::MoveEntries { entries, folder }) = &submission
            && let Err(error) = check_move_target(&self.workspace, entries, folder)
        {
            prompt.prompt.set_error(Some(Cow::Owned(error)));
            return Vec::new();
        }
        self.close_prompt();
        match submission {
            Submission::Effect(effect) => vec![effect],
            Submission::SetDue { list, task, due } => self.set_due(&list, task, due),
            Submission::SetRecurrence {
                list,
                task,
                recurrence,
            } => self.set_recurrence(&list, task, recurrence),
        }
    }

    /// Closes the open prompt without effect and discards a pending key sequence.
    pub(super) fn close_prompt(&mut self) {
        match self.mode {
            Mode::Prompt(_) => {
                self.resolver.reset();
                self.mode = Mode::Normal;
            }
            Mode::Normal
            | Mode::Help(_)
            | Mode::Confirm(_)
            | Mode::InlineEdit(_)
            | Mode::Search(_)
            | Mode::Visual => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use jiff::civil::date;
    use rstest::rstest;
    use tempfile::TempDir;

    use super::*;
    use crate::app::state::tests::app_in;
    use crate::config::keymap::action::{Action, Context};
    use crate::config::keymap::resolver::Resolution;

    const TODAY: Date = date(2026, 11, 24);

    fn new_list(parent: &str) -> PromptKind {
        PromptKind::NewList {
            parent: PathBuf::from(parent),
        }
    }

    fn new_folder(parent: &str) -> PromptKind {
        PromptKind::NewFolder {
            parent: PathBuf::from(parent),
        }
    }

    fn rename(kind: EntryKind, path: &str) -> PromptKind {
        PromptKind::Rename {
            kind,
            path: PathBuf::from(path),
        }
    }

    fn prompt_of(app: &App) -> &PromptOverlay {
        match app.mode() {
            Mode::Prompt(prompt) => prompt,
            mode => panic!("no prompt is open, the mode is {mode:?}"),
        }
    }

    /// Feeds the first key of a sequence of the lists pane, so that a sequence is pending.
    fn start_key_sequence(app: &mut App) {
        let _: Resolution = app
            .resolver
            .feed(&app.keymap, Context::Lists, "g".parse().unwrap());
    }

    #[rstest]
    #[case::list_at_the_top_level(new_list(""), "New list", "in the workspace folder")]
    #[case::list_in_a_folder(new_list("Home/Garden"), "New list", "in Home/Garden")]
    #[case::folder_in_a_folder(new_folder("Home"), "New folder", "in Home")]
    #[case::rename_list(rename(EntryKind::List, "Home/Chores.md"), "Rename list", "in Home")]
    #[case::rename_folder(
        rename(EntryKind::Folder, "Work"),
        "Rename folder",
        "in the workspace folder"
    )]
    #[case::move_to_folder(
        PromptKind::MoveToFolder { entries: Vec::new() },
        "Move to folder",
        MOVE_HINT
    )]
    fn open_shows_the_title_and_the_target_folder(
        #[case] kind: PromptKind,
        #[case] expected_title: &str,
        #[case] expected_hint: &str,
    ) {
        let expected = Prompt::new(expected_title.to_owned(), TextInput::new())
            .with_hint(expected_hint.to_owned());

        let system_under_test = PromptOverlay::open(kind, TextInput::new(), TODAY);

        assert_eq!(system_under_test.prompt(), &expected);
    }

    #[rstest]
    #[case::list(new_list("Home"), EntryKind::List)]
    #[case::folder(new_folder("Home"), EntryKind::Folder)]
    fn valid_name_becomes_a_create_effect(#[case] kind: PromptKind, #[case] expected: EntryKind) {
        let expected = Effect::CreateEntry {
            kind: expected,
            parent: PathBuf::from("Home"),
            name: EntryName::try_from("Groceries").unwrap(),
        };

        let system_under_test = kind.submit(" Groceries.md ", TODAY);

        assert_eq!(system_under_test, Ok(Submission::Effect(expected)));
    }

    #[test]
    fn valid_name_becomes_a_rename_effect() {
        let kind = rename(EntryKind::List, "Home/Chores.md");
        let expected = Effect::RenameEntry {
            path: PathBuf::from("Home/Chores.md"),
            name: EntryName::try_from("Tasks").unwrap(),
        };

        let system_under_test = kind.submit("Tasks", TODAY);

        assert_eq!(system_under_test, Ok(Submission::Effect(expected)));
    }

    #[rstest]
    #[case::folder(" Home/Garden/ ", "Home/Garden")]
    #[case::root("  ", "")]
    #[case::slash_only("/", "")]
    fn folder_text_becomes_a_move_effect(#[case] text: &str, #[case] expected_folder: &str) {
        let entries = vec![EntryKey::Folder(PathBuf::from("Work"))];
        let kind = PromptKind::MoveToFolder {
            entries: entries.clone(),
        };
        let expected = Effect::MoveEntries {
            entries,
            folder: PathBuf::from(expected_folder),
        };

        let system_under_test = kind.submit(text, TODAY);

        assert_eq!(system_under_test, Ok(Submission::Effect(expected)));
    }

    #[test]
    fn invalid_name_becomes_its_reason() {
        let kind = new_list("");
        let expected = "names must not contain ':'";

        let system_under_test = kind.submit("a:b", TODAY);

        assert_eq!(system_under_test, Err(Cow::Owned(expected.to_owned())));
    }

    #[test]
    fn opening_a_prompt_discards_a_pending_key_sequence() {
        let directory = TempDir::new().unwrap();
        let mut app = app_in(directory.path());
        start_key_sequence(&mut app);
        app.open_prompt(new_list(""), TextInput::new());

        let system_under_test = app.pending_keys();

        assert_eq!(system_under_test, []);
    }

    #[rstest]
    #[case::cancel(Action::Cancel)]
    #[case::submit(Action::Submit)]
    fn closing_a_prompt_discards_a_pending_key_sequence(#[case] action: Action) {
        let directory = TempDir::new().unwrap();
        let mut app = app_in(directory.path());
        app.open_prompt(new_folder(""), TextInput::new());
        let Mode::Prompt(prompt) = &mut app.mode else {
            unreachable!("the prompt was just opened");
        };
        prompt.paste("Work", TODAY);
        start_key_sequence(&mut app);
        let _effects = app.update(action);

        let system_under_test = app.pending_keys();

        assert_eq!(system_under_test, []);
        assert!(matches!(app.mode(), Mode::Normal));
    }

    #[test]
    fn invalid_submit_keeps_the_prompt_open_without_effect() {
        let directory = TempDir::new().unwrap();
        let mut app = app_in(directory.path());
        app.open_prompt(new_list(""), TextInput::new());

        let system_under_test = app.update(Action::Submit);

        assert_eq!(system_under_test, []);
        assert_eq!(prompt_of(&app).prompt().error(), Some("the name is empty"));
    }

    #[test]
    fn unchanged_text_keeps_the_error() {
        let directory = TempDir::new().unwrap();
        let mut app = app_in(directory.path());
        app.open_prompt(new_list(""), TextInput::new());
        let _effects = app.update(Action::Submit);
        let Mode::Prompt(prompt) = &mut app.mode else {
            unreachable!("an invalid name keeps the prompt open");
        };
        prompt.press("backspace".parse().unwrap(), TODAY);

        let system_under_test = prompt_of(&app).prompt().error();

        assert_eq!(system_under_test, Some("the name is empty"));
    }

    #[test]
    fn closing_without_a_prompt_keeps_the_mode() {
        let directory = TempDir::new().unwrap();
        let mut app = app_in(directory.path());
        app.open_help();
        app.close_prompt();

        let system_under_test = app.mode();

        assert!(matches!(system_under_test, Mode::Help(_)));
    }
}

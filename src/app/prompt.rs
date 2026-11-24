//! The state of an input prompt in [`Mode::Prompt`]: what the typed text is for, the
//! [`Prompt`] popup that edits it, and what submitting it does.
//!
//! The typed text is validated here, without touching the disk; a valid text becomes an
//! [`Effect`] whose result comes back to the [`App`].

use std::borrow::Cow;
use std::path::{Path, PathBuf};

use crate::app::effect::Effect;
use crate::app::state::{App, Mode};
use crate::config::keymap::key::KeyChord;
use crate::storage::entries::EntryName;
use crate::storage::workspace::EntryKind;
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
        }
    }

    /// Returns the hint below the input: the folder of the new or renamed entry.
    fn hint(&self) -> Cow<'static, str> {
        match self {
            Self::NewList { parent } | Self::NewFolder { parent } => location_hint(parent),
            Self::Rename { path, .. } => location_hint(path.parent().unwrap_or(Path::new(""))),
        }
    }

    /// Parses `text` and returns the effect that performs what the prompt asks for.
    fn submit(&self, text: &str) -> Result<Effect, Cow<'static, str>> {
        let name = EntryName::try_from(text).map_err(|error| error.to_string())?;
        Ok(match self {
            Self::NewList { parent } => Effect::CreateEntry {
                kind: EntryKind::List,
                parent: parent.clone(),
                name,
            },
            Self::NewFolder { parent } => Effect::CreateEntry {
                kind: EntryKind::Folder,
                parent: parent.clone(),
                name,
            },
            Self::Rename { path, .. } => Effect::RenameEntry {
                path: path.clone(),
                name,
            },
        })
    }
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
    /// Opens a prompt for `kind` that edits `input`.
    fn open(kind: PromptKind, input: TextInput) -> Self {
        let prompt = Prompt::new(kind.title(), input).with_hint(kind.hint());
        Self { kind, prompt }
    }

    /// Returns the popup with the typed text and the error, as the view draws it.
    #[must_use]
    pub fn prompt(&self) -> &Prompt {
        &self.prompt
    }

    /// Edits the text with a key press that no action is bound to; a change hides the error.
    pub(super) fn press(&mut self, chord: KeyChord) {
        match self.prompt.input_mut().press(chord) {
            TextChange::Changed => self.prompt.set_error(None),
            TextChange::Unchanged => {}
        }
    }

    /// Inserts pasted text and hides the error.
    pub(super) fn paste(&mut self, text: &str) {
        self.prompt.input_mut().insert_str(text);
        self.prompt.set_error(None);
    }

    /// Returns the effect the typed text asks for, or shows why the text is invalid and
    /// returns `None`.
    fn submit(&mut self) -> Option<Effect> {
        match self.kind.submit(self.prompt.input().value()) {
            Ok(effect) => Some(effect),
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
        self.mode = Mode::Prompt(PromptOverlay::open(kind, input));
    }

    /// Closes the open prompt and returns the effect of its text; an invalid text keeps the
    /// prompt open and shows why.
    pub(super) fn submit_prompt(&mut self) -> Vec<Effect> {
        let Mode::Prompt(prompt) = &mut self.mode else {
            return Vec::new();
        };
        let Some(effect) = prompt.submit() else {
            return Vec::new();
        };
        self.close_prompt();
        vec![effect]
    }

    /// Closes the open prompt without effect and discards a pending key sequence.
    pub(super) fn close_prompt(&mut self) {
        match self.mode {
            Mode::Prompt(_) => {
                self.resolver.reset();
                self.mode = Mode::Normal;
            }
            Mode::Normal | Mode::Help(_) | Mode::Confirm(_) => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;
    use tempfile::TempDir;

    use super::*;
    use crate::app::state::tests::app_in;
    use crate::config::keymap::action::{Action, Context};
    use crate::config::keymap::resolver::Resolution;

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
    fn open_shows_the_title_and_the_target_folder(
        #[case] kind: PromptKind,
        #[case] expected_title: &str,
        #[case] expected_hint: &str,
    ) {
        let expected = Prompt::new(expected_title.to_owned(), TextInput::new())
            .with_hint(expected_hint.to_owned());

        let system_under_test = PromptOverlay::open(kind, TextInput::new());

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

        let system_under_test = kind.submit(" Groceries.md ");

        assert_eq!(system_under_test, Ok(expected));
    }

    #[test]
    fn valid_name_becomes_a_rename_effect() {
        let kind = rename(EntryKind::List, "Home/Chores.md");
        let expected = Effect::RenameEntry {
            path: PathBuf::from("Home/Chores.md"),
            name: EntryName::try_from("Tasks").unwrap(),
        };

        let system_under_test = kind.submit("Tasks");

        assert_eq!(system_under_test, Ok(expected));
    }

    #[test]
    fn invalid_name_becomes_its_reason() {
        let kind = new_list("");
        let expected = "names must not contain ':'";

        let system_under_test = kind.submit("a:b");

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
        prompt.paste("Work");
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
        prompt.press("backspace".parse().unwrap());

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

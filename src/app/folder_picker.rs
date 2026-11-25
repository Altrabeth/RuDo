//! The folder picker ([`Mode::FolderPicker`]): asks for the folder that receives moved lists
//! and folders, or entries of the bin whose original folder is unknown, with a fuzzy
//! [`Picker`] over the folders of the workspace.
//!
//! The candidates are the workspace root, shown as `/`, and every folder the lists pane can
//! show: hidden folders only while hidden entries are shown, and never the bin. They are
//! collected when the picker opens; the choice is checked against the workspace again on
//! submit, so a folder removed in the meantime is reported instead of used.

use std::mem;
use std::path::{Path, PathBuf};

use crate::app::effect::Effect;
use crate::app::entry_visibility::HiddenEntries;
use crate::app::lists::{EntryKey, TreeRow, check_move_target, tree_items};
use crate::app::picker::{Picker, PickerCandidate, PickerPrompt, path_name_start};
use crate::app::state::{App, Mode};
use crate::paths::portable;
use crate::storage::workspace::{Entry, Workspace};

/// How the workspace root is shown and completed.
const ROOT_TEXT: &str = "/";
/// Explains why the bin asks for a folder.
const RESTORE_HINT: &str = "the original folder is unknown";

/// What the chosen folder receives.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum FolderPurpose {
    /// The lists and folders to move into the chosen folder.
    Move(Vec<EntryKey>),
    /// The entries of the bin at these locations, relative to the bin, to restore; those of
    /// unknown origin go into the chosen folder, the others back to their original path.
    RestoreInto(Vec<PathBuf>),
}

impl FolderPurpose {
    /// Returns the title of the popup.
    const fn title(&self) -> &'static str {
        match self {
            Self::Move(_) => "Move to folder",
            Self::RestoreInto(_) => "Restore into folder",
        }
    }

    /// Returns the hint shown below the results while there is no error.
    const fn hint(&self) -> Option<&'static str> {
        match self {
            Self::Move(_) => None,
            Self::RestoreInto(_) => Some(RESTORE_HINT),
        }
    }

    /// Returns the lists and folders that must not receive themselves.
    fn moved_entries(&self) -> &[EntryKey] {
        match self {
            Self::Move(entries) => entries,
            Self::RestoreInto(_) => &[],
        }
    }
}

/// A folder of the workspace offered by the folder picker.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FolderCandidate {
    /// Relative to the workspace root; empty for the root.
    path: PathBuf,
    /// The path with `/` on every platform, or [`ROOT_TEXT`] for the root.
    text: String,
}

impl FolderCandidate {
    /// Creates the candidate of the folder at `path`, relative to the workspace root and
    /// empty for the root.
    fn new(path: &Path) -> Self {
        let text = if path.as_os_str().is_empty() {
            ROOT_TEXT.to_owned()
        } else {
            portable(path).to_string()
        };
        Self {
            path: path.to_path_buf(),
            text,
        }
    }

    /// Returns the path of the folder relative to the workspace root, empty for the root.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl PickerCandidate for FolderCandidate {
    fn text(&self) -> &str {
        &self.text
    }

    /// The name of a folder is the last component of its path.
    fn name_start(&self) -> usize {
        path_name_start(&self.text)
    }
}

/// Returns the folders of `workspace` the lists pane shows with `hidden_entries`, in display
/// order after the root.
fn folder_candidates(workspace: &Workspace, hidden_entries: HiddenEntries) -> Vec<FolderCandidate> {
    let folders = tree_items(workspace, hidden_entries)
        .into_iter()
        .filter_map(|item| match item.row() {
            TreeRow::Entry(Entry::Folder(folder)) => Some(FolderCandidate::new(folder.path())),
            TreeRow::Entry(Entry::List(_)) | TreeRow::Bin(_) => None,
        });
    std::iter::once(FolderCandidate::new(Path::new("")))
        .chain(folders)
        .collect()
}

/// Returns the folder named by typed `text`, relative to the workspace root: surrounding
/// whitespace and slashes are ignored, so that empty text names the root.
fn folder_path(text: &str) -> PathBuf {
    PathBuf::from(text.trim().trim_matches('/'))
}

/// An open folder picker: what the folder is for and the picker with the folders.
#[derive(Debug, Clone)]
pub struct FolderPicker {
    purpose: FolderPurpose,
    prompt: PickerPrompt<FolderCandidate>,
}

impl FolderPicker {
    /// Returns the title of the popup.
    #[must_use]
    pub const fn title(&self) -> &'static str {
        self.purpose.title()
    }

    /// Returns the hint shown while there is no error.
    #[must_use]
    pub const fn hint(&self) -> Option<&'static str> {
        self.purpose.hint()
    }

    /// Returns the picker with the typed text and the matching folders.
    #[must_use]
    pub const fn picker(&self) -> &Picker<FolderCandidate> {
        self.prompt.picker()
    }

    /// Returns why the last choice was refused, until the text or the selection changes.
    #[must_use]
    pub fn error(&self) -> Option<&str> {
        self.prompt.error()
    }

    /// Returns the picker with its error, to type into.
    pub(super) const fn prompt_mut(&mut self) -> &mut PickerPrompt<FolderCandidate> {
        &mut self.prompt
    }

    /// Returns the chosen folder relative to the workspace root: the selected match, or the
    /// typed path if nothing matches.
    fn chosen_folder(&self) -> PathBuf {
        let picker = self.prompt.picker();
        picker.selected().map_or_else(
            || folder_path(picker.input().value()),
            |candidate| candidate.path().to_path_buf(),
        )
    }
}

impl App {
    /// Opens the folder picker for `purpose` over the folders the lists pane can show, and
    /// discards a pending key sequence.
    pub(super) fn open_folder_picker(&mut self, purpose: FolderPurpose) {
        self.resolver.reset();
        let candidates = folder_candidates(&self.workspace, self.lists.hidden_entries());
        self.mode = Mode::FolderPicker(FolderPicker {
            purpose,
            prompt: PickerPrompt::new(candidates),
        });
    }

    /// Closes the folder picker and returns the effects of the chosen folder; a folder that
    /// does not exist or cannot receive the entries keeps the picker open and shows why.
    pub(super) fn submit_folder_picker(&mut self) -> Vec<Effect> {
        let Mode::FolderPicker(folder_picker) = &mut self.mode else {
            return Vec::new();
        };
        let folder = folder_picker.chosen_folder();
        if let Err(error) = check_move_target(
            &self.workspace,
            folder_picker.purpose.moved_entries(),
            &folder,
        ) {
            folder_picker.prompt.refuse(error);
            return Vec::new();
        }
        // The folder picker was open above, so the mode is always taken from it here.
        let Mode::FolderPicker(FolderPicker { purpose, .. }) = mem::take(&mut self.mode) else {
            return Vec::new();
        };
        self.resolver.reset();
        match purpose {
            FolderPurpose::Move(entries) => vec![Effect::MoveEntries { entries, folder }],
            FolderPurpose::RestoreInto(locations) => self.restore_into(&locations, &folder),
        }
    }

    /// Closes the folder picker without effect and discards a pending key sequence.
    pub(super) fn close_folder_picker(&mut self) {
        if let Mode::FolderPicker(_) = self.mode {
            self.resolver.reset();
            self.mode = Mode::Normal;
        }
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use rstest::rstest;
    use tempfile::TempDir;

    use super::*;
    use crate::app::state::tests::app_in;
    use crate::config::IndentStyle;
    use crate::config::keymap::action::Action;
    use crate::storage::bin::BinPath;

    /// A workspace with the folders `Home/Garden`, `Work` and the hidden `.archive`, a list in
    /// each, and an entry in the bin.
    fn workspace(root: &TempDir) -> Workspace {
        for list in [
            "Home/Garden/Seeds.md",
            "Work/Plan.md",
            ".archive/Old.md",
            "Inbox.md",
            ".trash/Gone/Note.md",
        ] {
            let path = root.path().join(list);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, "- [ ] a\n").unwrap();
        }
        Workspace::scan(root.path(), BinPath::default(), IndentStyle::default()).unwrap()
    }

    fn texts(candidates: &[FolderCandidate]) -> Vec<&str> {
        candidates.iter().map(PickerCandidate::text).collect()
    }

    fn folder_picker_of(app: &App) -> &FolderPicker {
        match app.mode() {
            Mode::FolderPicker(folder_picker) => folder_picker,
            mode => panic!("no folder picker is open, the mode is {mode:?}"),
        }
    }

    #[rstest]
    #[case::hidden(HiddenEntries::Hidden, &["/", "Home", "Home/Garden", "Work"])]
    #[case::shown(HiddenEntries::Shown, &["/", ".archive", "Home", "Home/Garden", "Work"])]
    fn candidates_are_the_root_and_the_shown_folders_without_the_bin(
        #[case] hidden_entries: HiddenEntries,
        #[case] expected: &[&str],
    ) {
        let root = TempDir::new().unwrap();
        let workspace = workspace(&root);

        let system_under_test = folder_candidates(&workspace, hidden_entries);

        assert_eq!(texts(&system_under_test), expected);
    }

    #[rstest]
    #[case::root_has_no_name("", 1)]
    #[case::top_level("Work", 0)]
    #[case::nested("Home/Garden", 5)]
    fn name_starts_after_the_last_slash(#[case] path: &str, #[case] expected: usize) {
        let candidate = FolderCandidate::new(Path::new(path));

        let system_under_test = candidate.name_start();

        assert_eq!(system_under_test, expected);
    }

    #[rstest]
    #[case::folder(" Home/Garden/ ", "Home/Garden")]
    #[case::root("  ", "")]
    #[case::slash_only("/", "")]
    fn typed_text_names_a_folder(#[case] text: &str, #[case] expected: &str) {
        let system_under_test = folder_path(text);

        assert_eq!(system_under_test, Path::new(expected));
    }

    #[rstest]
    #[case::move_entries(FolderPurpose::Move(Vec::new()), "Move to folder", None)]
    #[case::restore(
        FolderPurpose::RestoreInto(Vec::new()),
        "Restore into folder",
        Some(RESTORE_HINT)
    )]
    fn purpose_names_the_popup(
        #[case] purpose: FolderPurpose,
        #[case] expected_title: &str,
        #[case] expected_hint: Option<&str>,
    ) {
        let system_under_test = (purpose.title(), purpose.hint());

        assert_eq!(system_under_test, (expected_title, expected_hint));
    }

    #[test]
    fn selected_folder_becomes_a_move_effect() {
        let root = TempDir::new().unwrap();
        let _: Workspace = workspace(&root);
        let mut app = app_in(root.path());
        let entries = vec![EntryKey::Folder(PathBuf::from("Work"))];
        app.open_folder_picker(FolderPurpose::Move(entries.clone()));
        let _: Vec<Effect> = app.handle_paste("gar");
        let expected = vec![Effect::MoveEntries {
            entries,
            folder: PathBuf::from("Home/Garden"),
        }];

        let system_under_test = app.update(Action::Submit);

        assert_eq!(system_under_test, expected);
        assert!(matches!(app.mode(), Mode::Normal));
    }

    #[test]
    fn refused_folder_keeps_the_picker_open_until_the_next_change() {
        let root = TempDir::new().unwrap();
        let _: Workspace = workspace(&root);
        let mut app = app_in(root.path());
        app.open_folder_picker(FolderPurpose::Move(vec![EntryKey::Folder(PathBuf::from(
            "Home",
        ))]));
        let _: Vec<Effect> = app.handle_paste("Home/Garden");
        let effects = app.update(Action::Submit);
        let refused = folder_picker_of(&app).error().map(str::to_owned);

        let system_under_test = {
            let _: Vec<Effect> = app.update(Action::PickerNext);
            folder_picker_of(&app).error()
        };

        assert_eq!(effects, []);
        assert_eq!(
            refused.as_deref(),
            Some("cannot move folder Home into itself")
        );
        assert_eq!(system_under_test, None);
    }

    #[test]
    fn closing_without_a_folder_picker_keeps_the_mode() {
        let root = TempDir::new().unwrap();
        let mut app = app_in(root.path());
        app.open_help();
        app.close_folder_picker();

        let system_under_test = app.mode();

        assert!(matches!(system_under_test, Mode::Help(_)));
    }
}

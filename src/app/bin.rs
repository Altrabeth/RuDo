//! The bin view of the lists pane: the lists and folders in the bin with their original path
//! and deletion date, restoring them, deleting them permanently and emptying the bin.
//!
//! [`BinView`] never copies the bin: [`bin_items`] lists the entries of the workspace whenever
//! the view needs them. A list in the bin is only ever shown, never edited: the keys of the bin
//! ([`Context::Bin`](crate::config::keymap::action::Context::Bin)) hold no action on tasks and
//! cannot move the focus to the tasks pane, and the tasks pane renders the selected entry from
//! the bin, not the active list, so no edit can reach [`App::edit_list`].

use std::path::{Path, PathBuf};

use crate::app::confirm::ConfirmKind;
use crate::app::effect::{Effect, error_report};
use crate::app::lists::{count_lists_and_tasks, counted, entry_description};
use crate::app::prompt::PromptKind;
use crate::app::state::{App, Focus};
use crate::app::status::StatusMessage;
use crate::app::task_delete::counted_tasks;
use crate::config::keymap::action::Action;
use crate::storage::bin::{BinEntry, EmptiedBin};
use crate::storage::workspace::Workspace;
use crate::ui::outline::{Outcome, Outline, OutlineCommand, OutlineItem};
use crate::ui::text_input::TextInput;

/// Ends every question about a permanent deletion.
const CANNOT_BE_UNDONE: &str = "This cannot be undone.";
/// Shown when an action of the bin finds its entry gone.
const ENTRY_GONE: &str = "the entry is no longer in the bin";

/// An entry of the bin with its key, its path relative to the bin.
#[derive(Debug)]
pub struct BinItem<'workspace> {
    entry: &'workspace BinEntry,
    key: PathBuf,
}

impl<'workspace> BinItem<'workspace> {
    /// Returns the entry of the bin of this row.
    #[must_use]
    pub fn entry(&self) -> &'workspace BinEntry {
        self.entry
    }
}

impl OutlineItem for BinItem<'_> {
    type Key = PathBuf;

    fn key(&self) -> &PathBuf {
        &self.key
    }

    /// The bin view is flat: entries are restored and deleted as a whole.
    fn depth(&self) -> usize {
        0
    }
}

/// Returns the entries of the bin of `workspace` in display order, the most recently deleted
/// first, ready for an [`Outline`].
#[must_use]
pub fn bin_items(workspace: &Workspace) -> Vec<BinItem<'_>> {
    workspace
        .bin_entries()
        .iter()
        .map(|entry| BinItem {
            entry,
            key: entry.location().to_path_buf(),
        })
        .collect()
}

/// The bin view: the selection and scrolling of the entries in the bin, shown by the lists
/// pane instead of the tree while it is open.
#[derive(Debug, Clone, Default)]
pub struct BinView {
    outline: Outline<PathBuf>,
}

impl BinView {
    /// Opens the view on the bin of `workspace` with its first entry selected, in a viewport
    /// `viewport_height` rows high.
    pub(super) fn new(workspace: &Workspace, viewport_height: usize) -> Self {
        let mut view = Self::default();
        view.refresh(workspace, viewport_height);
        view
    }

    /// Returns the navigation state of the entries, keyed by their path relative to the bin.
    #[must_use]
    pub fn outline(&self) -> &Outline<PathBuf> {
        &self.outline
    }

    /// Keeps the selection on an entry of the bin of `workspace` inside a viewport
    /// `viewport_height` rows high, after the bin changed or the terminal was resized.
    pub(super) fn refresh(&mut self, workspace: &Workspace, viewport_height: usize) {
        self.outline.refresh(&bin_items(workspace), viewport_height);
    }

    /// Performs the navigation `action` on the entries of the bin of `workspace`.
    pub(super) fn update(&mut self, workspace: &Workspace, action: Action, viewport_height: usize) {
        if let Some(command) = OutlineCommand::from_action(action) {
            // Moving past the first or last entry changes nothing, which is fine.
            let _: Outcome = self
                .outline
                .apply(&bin_items(workspace), command, viewport_height);
        }
    }
}

impl App {
    /// Returns the entry selected in the bin view, or `None` while the bin is not open or
    /// empty.
    #[must_use]
    pub fn selected_bin_entry(&self) -> Option<&BinEntry> {
        let location = self.lists.bin_view()?.outline().selected()?;
        self.workspace.bin_entry(location)
    }

    /// Restores the selected entry of the bin to its original path, or asks for the folder
    /// that receives it if the original path is unknown.
    pub(super) fn restore_from_bin(&mut self) -> Vec<Effect> {
        let Some(entry) = self.selected_bin_entry() else {
            return Vec::new();
        };
        if let Some(trashed) = entry.trashed() {
            return vec![Effect::RestoreEntries(vec![trashed.clone()])];
        }
        let kind = PromptKind::RestoreInto {
            location: entry.location().to_path_buf(),
        };
        self.open_prompt(kind, TextInput::new());
        Vec::new()
    }

    /// Restores the entry of the bin at `location`, relative to the bin, into the existing
    /// `folder`, relative to the workspace root, keeping its name.
    pub(super) fn restore_into(&mut self, location: &Path, folder: &Path) -> Vec<Effect> {
        if let Some(entry) = self.workspace.bin_entry(location) {
            return vec![Effect::RestoreEntries(vec![entry.restored_into(folder)])];
        }
        self.show_message(StatusMessage::error(ENTRY_GONE));
        Vec::new()
    }

    /// Asks to confirm deleting the selected entry of the bin permanently.
    pub(super) fn ask_to_delete_from_bin(&mut self) {
        let Some(entry) = self.selected_bin_entry() else {
            return;
        };
        let message = format!(
            "Delete {} permanently? {CANNOT_BE_UNDONE}",
            entry_description(entry.entry())
        );
        let kind = ConfirmKind::DeleteFromBin(entry.location().to_path_buf());
        self.open_confirm(kind, message);
    }

    /// Asks to confirm deleting everything in the bin permanently, with the number of lists
    /// and tasks it holds.
    pub(super) fn ask_to_empty_bin(&mut self) {
        if self.lists.bin_view().is_none() {
            return;
        }
        let message = empty_bin_question(self.workspace.bin_entries());
        self.open_confirm(ConfirmKind::EmptyBin, message);
    }

    /// Shows the tree again instead of the bin, and the active list in the tasks pane.
    pub(super) fn close_bin(&mut self) {
        self.lists.close_bin();
        self.refresh_tasks();
    }

    /// Brings the bin view in line with the bin after entries left or entered it, and closes
    /// it once the bin is empty.
    pub(super) fn bin_changed(&mut self) {
        if self.workspace.bin_entries().is_empty() {
            self.lists.close_bin();
        }
        let viewport_height = self.viewport_height(Focus::Lists);
        self.lists.refresh(&self.workspace, viewport_height);
        self.refresh_tasks();
    }
}

/// Asks whether to delete `entries`, the whole bin, permanently, with the number of lists in
/// it, at any depth, and their tasks: `Delete 12 lists with 340 tasks permanently? This cannot
/// be undone.`; a bin without lists counts its folders.
///
/// The bin view and `rudo bin empty` ask the same question.
#[must_use]
pub fn empty_bin_question(entries: &[BinEntry]) -> String {
    let (lists, tasks) = count_lists_and_tasks(entries.iter().map(BinEntry::entry));
    let what = if lists == 0 {
        counted(entries.len(), "folder", "folders")
    } else {
        format!(
            "{} with {}",
            counted(lists, "list", "lists"),
            counted_tasks(tasks)
        )
    };
    format!("Delete {what} permanently? {CANNOT_BE_UNDONE}")
}

/// Reports what [`empty_bin`](crate::storage::bin::empty_bin) did: `emptied the bin`, or the
/// entries that could not be deleted in one error, such as `emptied 298 of 300 entries of the
/// bin; 2 failed: ...`.
///
/// The bin view and `rudo bin empty` report the same.
#[must_use]
pub fn emptied_message(emptied: &EmptiedBin) -> StatusMessage {
    let failures = emptied.failures();
    if failures.is_empty() {
        return StatusMessage::info("emptied the bin");
    }
    let reports: Vec<String> = failures.iter().map(|error| error_report(error)).collect();
    StatusMessage::error(format!(
        "emptied {} of {} entries of the bin; {} failed: {}",
        emptied.entries() - failures.len(),
        emptied.entries(),
        failures.len(),
        reports.join("; ")
    ))
}

#[cfg(test)]
mod tests {
    use std::fs;

    use rstest::rstest;
    use tempfile::TempDir;

    use super::*;
    use crate::app::status::Severity;
    use crate::config::IndentStyle;
    use crate::storage::bin::{BinPath, empty_bin};

    /// Scans a workspace whose bin holds the files `(path, content)`, relative to the bin, and
    /// the folders (paths ending in `/`).
    fn workspace_with_bin(root: &TempDir, files: &[(&str, &str)]) -> Workspace {
        for (path, content) in files {
            let absolute = root.path().join(".trash").join(path);
            if path.ends_with('/') {
                fs::create_dir_all(absolute).unwrap();
            } else {
                fs::create_dir_all(absolute.parent().unwrap()).unwrap();
                fs::write(absolute, content).unwrap();
            }
        }
        Workspace::scan(root.path(), BinPath::default(), IndentStyle::default()).unwrap()
    }

    #[rstest]
    #[case::lists(
        &[("a.md", "- [ ] a\n"), ("Work/b.md", "- [ ] b\n- [x] c\n")],
        "Delete 2 lists with 3 tasks permanently? This cannot be undone."
    )]
    #[case::one_list(&[("a.md", "- [ ] a\n")], "Delete 1 list with 1 task permanently? This cannot be undone.")]
    #[case::folders(&[("Old/", ""), ("Older/", "")], "Delete 2 folders permanently? This cannot be undone.")]
    fn empty_bin_question_counts_lists_and_tasks_at_any_depth(
        #[case] files: &[(&str, &str)],
        #[case] expected: &str,
    ) {
        let root = TempDir::new().unwrap();
        let workspace = workspace_with_bin(&root, files);

        let system_under_test = empty_bin_question(workspace.bin_entries());

        assert_eq!(system_under_test, expected);
    }

    #[test]
    fn emptied_message_of_an_emptied_bin_is_information() {
        let root = TempDir::new().unwrap();
        workspace_with_bin(&root, &[("a.md", "")]);
        let expected = StatusMessage::info("emptied the bin");

        let system_under_test = emptied_message(&empty_bin(root.path(), &BinPath::default()));

        assert_eq!(system_under_test, expected);
    }

    #[cfg(unix)]
    #[test]
    fn emptied_message_counts_the_entries_that_could_not_be_deleted() {
        use std::os::unix::fs::PermissionsExt;

        /// Makes a folder read-only and writable again on drop, so that the temporary
        /// directory can be deleted.
        struct ReadOnly(PathBuf);

        impl Drop for ReadOnly {
            fn drop(&mut self) {
                fs::set_permissions(&self.0, fs::Permissions::from_mode(0o755)).unwrap();
            }
        }

        let root = TempDir::new().unwrap();
        workspace_with_bin(&root, &[("a.md", ""), ("Locked/b.md", "")]);
        let locked = root.path().join(".trash/Locked");
        fs::set_permissions(&locked, fs::Permissions::from_mode(0o555)).unwrap();
        let _read_only = ReadOnly(locked);
        let expected_start = "emptied 1 of 2 entries of the bin; 1 failed: cannot change ";

        let system_under_test = emptied_message(&empty_bin(root.path(), &BinPath::default()));

        assert_eq!(system_under_test.severity(), Severity::Error);
        assert!(
            system_under_test.text().starts_with(expected_start),
            "{}",
            system_under_test.text()
        );
        assert!(!root.path().join(".trash/a.md").exists());
    }

    #[test]
    fn bin_items_follow_the_order_of_the_bin() {
        let root = TempDir::new().unwrap();
        let workspace = workspace_with_bin(&root, &[("b.md", ""), ("a.md", ""), ("Old/", "")]);
        let expected = [
            PathBuf::from("Old"),
            PathBuf::from("a.md"),
            PathBuf::from("b.md"),
        ];

        let system_under_test: Vec<PathBuf> = bin_items(&workspace)
            .iter()
            .map(|item| item.key().clone())
            .collect();

        assert_eq!(system_under_test, expected);
    }

    #[test]
    fn new_view_selects_the_first_entry() {
        let root = TempDir::new().unwrap();
        let workspace = workspace_with_bin(&root, &[("a.md", ""), ("b.md", "")]);
        let expected = PathBuf::from("a.md");

        let system_under_test = BinView::new(&workspace, 10);

        assert_eq!(system_under_test.outline().selected(), Some(&expected));
    }
}

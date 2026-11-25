//! The bin view of the lists pane: the lists and folders in the bin with their original path
//! and deletion date, restoring them, deleting them permanently and emptying the bin.
//!
//! [`BinView`] never copies the bin: [`bin_items`] lists the entries of the workspace whenever
//! the view needs them. A list in the bin is only ever shown, never edited: the keys of the bin
//! ([`Context::Bin`](crate::config::keymap::action::Context::Bin)) hold no action on tasks and
//! cannot move the focus to the tasks pane, and the tasks pane renders the selected entry from
//! the bin, not the active list, so no edit can reach [`App::edit_list`].
//!
//! Restoring and deleting act on the entry under the cursor, or on the selection of visual
//! mode ([`Context::BinVisual`](crate::config::keymap::action::Context::BinVisual)), which
//! lives in the [`Outline`] of the view like the selection of the panes.

use std::path::{Path, PathBuf};

use crate::app::confirm::ConfirmKind;
use crate::app::effect::{Effect, error_report};
use crate::app::lists::{count_lists_and_tasks, counted, entry_description};
use crate::app::prompt::PromptKind;
use crate::app::state::{App, Focus, Mode};
use crate::app::status::StatusMessage;
use crate::app::task_delete::counted_tasks;
use crate::app::visual::selected_keys;
use crate::config::keymap::action::Action;
use crate::storage::bin::{BinEntry, EmptiedBin, TrashedEntry};
use crate::storage::workspace::{Entry, Workspace};
use crate::ui::outline::{FilterRole, Outcome, Outline, OutlineCommand, OutlineItem};
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

    pub(super) fn outline_mut(&mut self) -> &mut Outline<PathBuf> {
        &mut self.outline
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

    /// Restores the entries of the visual selection, or the selected entry, to their original
    /// paths in one step, or asks once for the folder that receives those whose original path
    /// is unknown.
    pub(super) fn restore_from_bin(&mut self) -> Vec<Effect> {
        let Some(locations) = self.chosen_bin_locations() else {
            return Vec::new();
        };
        let known: Option<Vec<TrashedEntry>> = locations
            .iter()
            .map(|location| self.workspace.bin_entry(location)?.trashed().cloned())
            .collect();
        if let Some(entries) = known {
            return vec![Effect::RestoreEntries(entries)];
        }
        self.open_prompt(PromptKind::RestoreInto { locations }, TextInput::new());
        Vec::new()
    }

    /// Restores the entries of the bin at `locations`, relative to the bin, in one step: those
    /// with a known original path to it, the others into the existing `folder`, relative to
    /// the workspace root, keeping their names.
    pub(super) fn restore_into(&mut self, locations: &[PathBuf], folder: &Path) -> Vec<Effect> {
        let entries: Vec<TrashedEntry> = locations
            .iter()
            .filter_map(|location| self.workspace.bin_entry(location))
            .map(|entry| {
                entry
                    .trashed()
                    .cloned()
                    .unwrap_or_else(|| entry.restored_into(folder))
            })
            .collect();
        if entries.is_empty() {
            self.show_message(StatusMessage::error(ENTRY_GONE));
            return Vec::new();
        }
        vec![Effect::RestoreEntries(entries)]
    }

    /// Asks to confirm deleting the entries of the visual selection, or the selected entry,
    /// permanently.
    pub(super) fn ask_to_delete_from_bin(&mut self) {
        let Some(locations) = self.chosen_bin_locations() else {
            return;
        };
        let entries: Vec<&BinEntry> = locations
            .iter()
            .filter_map(|location| self.workspace.bin_entry(location))
            .collect();
        let Some(message) = delete_from_bin_question(&entries) else {
            self.show_message(StatusMessage::error(ENTRY_GONE));
            return;
        };
        self.open_confirm(ConfirmKind::DeleteFromBin(locations), message);
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
    /// it once the bin is empty, which ends a selection in it.
    pub(super) fn bin_changed(&mut self) {
        if self.workspace.bin_entries().is_empty() {
            self.lists.close_bin();
        }
        let viewport_height = self.viewport_height(Focus::Lists);
        self.lists.refresh(&self.workspace, viewport_height);
        self.refresh_tasks();
        self.end_visual_mode_without_anchor();
    }

    /// Selects the entries of the bin at `locations`, in display order, in visual mode, so
    /// that the entries a restore of a selection skipped stay selected; they are contiguous
    /// because every entry between them left the bin.
    pub(super) fn select_in_bin(&mut self, locations: &[PathBuf]) {
        let (Some(first), Some(last)) = (locations.first(), locations.last()) else {
            return;
        };
        let viewport_height = self.viewport_height(Focus::Lists);
        let items = bin_items(&self.workspace);
        let Some(view) = self.lists.bin_view_mut() else {
            return;
        };
        let outline = view.outline_mut();
        // An entry that is gone is not selected, and the selection below stays unanchored.
        let _: Outcome = outline.select(&items, first, viewport_height);
        outline.start_selection();
        let _: Outcome = outline.select(&items, last, viewport_height);
        if outline.anchor() == Some(first) {
            self.mode = Mode::Visual;
        } else {
            outline.end_selection();
        }
    }

    /// Anchors a selection at the cursor of the bin view and returns whether it has one.
    pub(super) fn start_bin_selection(&mut self) -> bool {
        self.lists.bin_view_mut().is_some_and(|view| {
            let outline = view.outline_mut();
            outline.start_selection();
            outline.anchor().is_some()
        })
    }

    /// Ends the selection of the bin view, if it is open.
    pub(super) fn end_bin_selection(&mut self) {
        if let Some(view) = self.lists.bin_view_mut() {
            view.outline_mut().end_selection();
        }
    }

    /// Returns the locations of the entries of the bin in the visual selection, relative to
    /// the bin and in display order, or `None` without a selection in the bin view.
    pub(super) fn selected_bin_locations(&self) -> Option<Vec<PathBuf>> {
        let outline = self.lists.bin_view()?.outline();
        selected_keys(outline, &bin_items(&self.workspace), |_| FilterRole::Match)
    }

    /// Ends visual mode and returns the locations of the selected entries of the bin, or
    /// shows why there are none and returns `None`.
    fn take_selected_bin_locations(&mut self) -> Option<Vec<PathBuf>> {
        let selection = self.selected_bin_locations();
        self.end_visual_mode();
        self.nonempty_selection(selection)
    }

    /// Returns the locations an action of the bin acts on: those of the visual selection,
    /// which ends, or that of the selected entry; `None` if there are none.
    fn chosen_bin_locations(&mut self) -> Option<Vec<PathBuf>> {
        match self.mode {
            Mode::Visual => self.take_selected_bin_locations(),
            Mode::Normal
            | Mode::Help(_)
            | Mode::Prompt(_)
            | Mode::Confirm(_)
            | Mode::InlineEdit(_)
            | Mode::Search(_) => self
                .selected_bin_entry()
                .map(|entry| vec![entry.location().to_path_buf()]),
        }
    }
}

/// Asks whether to delete `entries` of the bin permanently, naming a single entry with what
/// it holds and counting several, lists at any depth: `Delete 12 lists with 340 tasks and 2
/// folders permanently? This cannot be undone.`; `None` without entries.
fn delete_from_bin_question(entries: &[&BinEntry]) -> Option<String> {
    let what = match entries {
        [] => return None,
        [entry] => entry_description(entry.entry()),
        several => {
            let (lists, tasks) = count_lists_and_tasks(several.iter().map(|entry| entry.entry()));
            let folders = several
                .iter()
                .filter(|entry| match entry.entry() {
                    Entry::Folder(_) => true,
                    Entry::List(_) => false,
                })
                .count();
            let lists = (lists > 0).then(|| lists_with_tasks(lists, tasks));
            let folders = (folders > 0).then(|| counted(folders, "folder", "folders"));
            let parts: Vec<String> = lists.into_iter().chain(folders).collect();
            parts.join(" and ")
        }
    };
    Some(format!("Delete {what} permanently? {CANNOT_BE_UNDONE}"))
}

/// Counts `lists` with their `tasks`: `12 lists with 340 tasks`.
fn lists_with_tasks(lists: usize, tasks: usize) -> String {
    format!(
        "{} with {}",
        counted(lists, "list", "lists"),
        counted_tasks(tasks)
    )
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
        lists_with_tasks(lists, tasks)
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
    #[cfg(unix)]
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

    #[rstest]
    #[case::nothing(&[], None)]
    #[case::one_list(
        &[("a.md", "- [ ] a\n")],
        Some("Delete list \"a\" with 1 task permanently? This cannot be undone.")
    )]
    #[case::lists_and_folders(
        &[("a.md", "- [ ] a\n"), ("Work/b.md", "- [ ] b\n- [x] c\n"), ("Old/", "")],
        Some("Delete 2 lists with 3 tasks and 2 folders permanently? This cannot be undone.")
    )]
    #[case::folders(
        &[("Old/", ""), ("Older/", "")],
        Some("Delete 2 folders permanently? This cannot be undone.")
    )]
    #[case::lists(
        &[("a.md", ""), ("b.md", "")],
        Some("Delete 2 lists with 0 tasks permanently? This cannot be undone.")
    )]
    fn delete_from_bin_question_names_one_entry_and_counts_several(
        #[case] files: &[(&str, &str)],
        #[case] expected: Option<&str>,
    ) {
        let root = TempDir::new().unwrap();
        let workspace = workspace_with_bin(&root, files);
        let entries: Vec<&BinEntry> = workspace.bin_entries().iter().collect();
        let expected = expected.map(str::to_owned);

        let system_under_test = delete_from_bin_question(&entries);

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

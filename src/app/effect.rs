//! The side effects the application asks for, and [`execute`], which performs them.
//!
//! The runtime and the tests share [`execute`], so the tests exercise the same file system
//! code as a running RuDo.

use std::error::Error;
use std::fmt::{self, Display, Formatter, Write};
use std::iter;
use std::num::NonZeroUsize;
use std::path::{Path, PathBuf};

use crate::app::history::{Direction, EntryOperation, HistoryEntry, Relocation};
use crate::app::lists::{EntryChange, EntryKey};
use crate::app::state::{App, Focus};
use crate::app::status::StatusMessage;
use crate::storage::bin::{TrashedEntry, move_to_bin, restore};
use crate::storage::entries::{
    EntryError, EntryName, create_folder, create_list, move_into, move_to, rename,
};
use crate::storage::fingerprint::Fingerprint;
use crate::storage::save::{SaveError, save};
use crate::storage::workspace::{EntryKind, List, ListPath, has_list_extension};

/// A side effect returned by [`App::update`] and performed by [`execute`].
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Effect {
    /// Write the document of the list at this path to its file.
    Save(ListPath),
    /// Create an empty list file or folder named `name` in the folder `parent`.
    CreateEntry {
        /// Whether to create a list file or a folder.
        kind: EntryKind,
        /// The folder that receives the entry, relative to the workspace root and empty for
        /// the root itself.
        parent: PathBuf,
        /// The name of the new entry, without the `.md` extension of list files.
        name: EntryName,
    },
    /// Rename the list file or folder at `path` to `name` in the same folder.
    RenameEntry {
        /// The entry to rename, relative to the workspace root.
        path: PathBuf,
        /// The new name, without the `.md` extension of list files.
        name: EntryName,
    },
    /// Delete these list files and folders by moving them into the bin, as one step of the
    /// undo history.
    ///
    /// Every deletion of lists and folders is this effect, that of a single entry too, so
    /// [`execute`] deletes them in one place and reports all failures in one message.
    DeleteEntries(Vec<EntryKey>),
    /// Move these list files and folders into `folder`, keeping their names.
    ///
    /// Every move of lists and folders is this effect, that of a single entry too.
    MoveEntries {
        /// The lists and folders to move.
        entries: Vec<EntryKey>,
        /// The folder that receives them, relative to the workspace root and empty for the
        /// root itself.
        folder: PathBuf,
    },
    /// Open the list at this path in the external editor, at the 1-based `line` if given,
    /// and reload it when the editor exits.
    ///
    /// The editor needs the terminal, so the runtime performs this effect, not [`execute`].
    OpenInEditor {
        /// The list to edit.
        list: ListPath,
        /// The line of the selected task.
        line: Option<NonZeroUsize>,
    },
    /// Read the list at this path from disk again, because another program changed its file;
    /// see [`App::reload_list`].
    ReloadList(ListPath),
    /// Scan the workspace folder again, because another program created, removed or renamed
    /// a list or folder; see [`App::rescan`].
    Rescan,
    /// Undo or redo a change of lists and folders by performing `operation`, which the undo
    /// history held; its reversal goes back into the history.
    StepEntries {
        /// Whether the step undoes or redoes the change.
        direction: Direction,
        /// The change of lists and folders that the step performs.
        operation: EntryOperation,
    },
}

/// Performs `effect` and reports its result to `app`.
///
/// Failures never abort: they become a [`StatusMessage`] on `app`. Effects that need the
/// terminal, such as [`Effect::OpenInEditor`], are performed by the runtime and only logged
/// here.
pub fn execute(app: &mut App, effect: Effect) {
    match effect {
        Effect::Save(list) => save_list(app, &list),
        Effect::CreateEntry { kind, parent, name } => create_entry(app, kind, &parent, &name),
        Effect::RenameEntry { path, name } => rename_entry(app, &path, &name),
        Effect::DeleteEntries(entries) => delete_entries(app, &entries),
        Effect::MoveEntries { entries, folder } => move_entries(app, &entries, &folder),
        Effect::OpenInEditor { list, .. } => {
            tracing::warn!(%list, "the editor can only be opened by the runtime");
        }
        Effect::ReloadList(list) => app.reload_list(&list),
        Effect::Rescan => app.rescan(),
        Effect::StepEntries {
            direction,
            operation,
        } => step_entries(app, direction, operation),
    }
}

/// Saves the list at `list` with the fingerprint of the content RuDo last loaded or saved.
fn save_list(app: &mut App, list: &ListPath) {
    let Some(Ok(loaded)) = app.workspace().list(list).map(List::content) else {
        tracing::warn!(%list, "list to save is not loaded any more");
        return;
    };
    let path = app.workspace().absolute_path(list);
    match save(&path, loaded.document(), loaded.fingerprint()) {
        Ok(fingerprint) => app.saved(list, fingerprint),
        Err(SaveError::ModifiedExternally { .. }) => app.save_conflicted(list),
        Err(error) => app.save_failed(&error),
    }
}

/// Creates the list file or folder `name` in the folder `parent` on disk.
fn create_entry(app: &mut App, kind: EntryKind, parent: &Path, name: &EntryName) {
    let directory = app.workspace().root().join(parent);
    let created = match kind {
        EntryKind::Folder => create_folder(&directory, name),
        EntryKind::List => create_list(&directory, name),
    };
    match created {
        Ok(path) => {
            app.entry_created(kind, parent, name);
            if let Some(file_name) = path.file_name() {
                let created = parent.join(file_name);
                app.record_entries(Some(EntryOperation::Trash(vec![created])));
            }
        }
        Err(error) => app.entry_failed(&error),
    }
}

/// Renames the list file or folder at `path` to `name` on disk.
fn rename_entry(app: &mut App, path: &Path, name: &EntryName) {
    match rename(&app.workspace().root().join(path), name) {
        Ok(renamed) => {
            app.entry_renamed(path, name);
            if let Some(file_name) = renamed.file_name() {
                let back = Relocation::new(path.with_file_name(file_name), path.to_path_buf());
                app.record_entries(Some(EntryOperation::Relocate(vec![back])));
            }
        }
        Err(error) => app.entry_failed(&error),
    }
}

/// Moves the list files and folders `entries` into the bin, removes the moved ones from the
/// workspace, records their restore as one undo step and reports the failures in one message.
fn delete_entries(app: &mut App, entries: &[EntryKey]) {
    let paths = entries
        .iter()
        .map(|entry| entry.path().to_path_buf())
        .collect();
    let performed = trash_entries(app, paths);
    app.record_entries(performed.reversal);
    app.entries_failed(EntriesOperation::Deleted, entries, &performed.failures);
}

/// Moves the list files and folders `entries` into `folder` on disk, follows the moved ones in
/// the workspace, records their move back as one undo step and reports the failures in one
/// message.
fn move_entries(app: &mut App, entries: &[EntryKey], folder: &Path) {
    let directory = app.workspace().root().join(folder);
    let mut moved_back = Vec::with_capacity(entries.len());
    let failures: Vec<EntryError> = entries
        .iter()
        .filter_map(|entry| {
            match move_into(&app.workspace().root().join(entry.path()), &directory) {
                Ok(moved) => {
                    app.entry_moved(entry.path(), folder);
                    let file_name = moved.file_name().unwrap_or_default();
                    let back = Relocation::new(folder.join(file_name), entry.path().to_path_buf());
                    moved_back.push(back);
                    None
                }
                Err(error) => Some(error),
            }
        })
        .collect();
    app.record_entries(operation(moved_back, EntryOperation::Relocate));
    app.entries_failed(EntriesOperation::Moved, entries, &failures);
}

/// Undoes or redoes a change of lists and folders by performing `operation`, and stores its
/// reversal for the opposite step.
fn step_entries(app: &mut App, direction: Direction, operation: EntryOperation) {
    let performed = match operation {
        EntryOperation::Trash(paths) => trash_entries(app, paths),
        EntryOperation::Restore { entries, active } => restore_entries(app, entries, active),
        EntryOperation::Relocate(relocations) => relocate_entries(app, relocations),
    };
    app.entries_stepped(direction, performed);
}

/// What performing an [`EntryOperation`] did, entry by entry.
#[derive(Debug)]
struct Performed {
    /// The operation that reverts the entries that changed, `None` if none did.
    reversal: Option<EntryOperation>,
    /// The part of the operation that failed, `None` if nothing failed.
    unperformed: Option<EntryOperation>,
    /// Why the entries of `unperformed` failed.
    failures: Vec<EntryError>,
}

/// Performs `step` on every item and returns what it returned for the items it succeeded on,
/// the items it failed on and why they failed.
fn attempt<Item, Done>(
    items: Vec<Item>,
    mut step: impl FnMut(&Item) -> Result<Done, EntryError>,
) -> (Vec<Done>, Vec<Item>, Vec<EntryError>) {
    let mut done = Vec::with_capacity(items.len());
    let mut failed = Vec::new();
    let mut failures = Vec::new();
    for item in items {
        match step(&item) {
            Ok(result) => done.push(result),
            Err(error) => {
                failed.push(item);
                failures.push(error);
            }
        }
    }
    (done, failed, failures)
}

/// Returns `items` as an operation built by `operation`, or `None` if there are none.
fn operation<Item>(
    items: Vec<Item>,
    operation: impl FnOnce(Vec<Item>) -> EntryOperation,
) -> Option<EntryOperation> {
    (!items.is_empty()).then(|| operation(items))
}

/// Moves the lists and folders at `paths` into the bin and removes them from the workspace.
fn trash_entries(app: &mut App, paths: Vec<PathBuf>) -> Performed {
    let active = app.lists().active().cloned();
    let (trashed, failed, failures) = attempt(paths, |path| {
        let workspace = app.workspace();
        let trashed = move_to_bin(workspace.root(), workspace.bin(), path)?;
        app.entry_deleted(path);
        Ok(trashed)
    });
    let active = active.filter(|list| {
        trashed
            .iter()
            .any(|entry| list.as_path().starts_with(entry.original()))
    });
    Performed {
        reversal: operation(trashed, |entries| EntryOperation::Restore {
            entries,
            active,
        }),
        unperformed: operation(failed, EntryOperation::Trash),
        failures,
    }
}

/// Moves the lists and folders `entries` from the bin back to their original paths, adds them
/// to the workspace and makes `active` the active list again.
fn restore_entries(
    app: &mut App,
    entries: Vec<TrashedEntry>,
    active: Option<ListPath>,
) -> Performed {
    let (restored, failed, failures) = attempt(entries, |entry| {
        let workspace = app.workspace();
        restore(workspace.root(), workspace.bin(), entry)?;
        Ok(entry.original().to_path_buf())
    });
    if !restored.is_empty() {
        app.entries_restored(&restored, active.as_ref());
    }
    Performed {
        reversal: operation(restored, EntryOperation::Trash),
        unperformed: operation(failed, |entries| EntryOperation::Restore {
            entries,
            active,
        }),
        failures,
    }
}

/// Moves each list or folder of `relocations` to its new path and follows it in the workspace.
fn relocate_entries(app: &mut App, relocations: Vec<Relocation>) -> Performed {
    let (reversed, failed, failures) = attempt(relocations, |relocation| {
        let root = app.workspace().root();
        move_to(&root.join(relocation.from()), &root.join(relocation.to()))?;
        app.entry_relocated_to(relocation.from(), relocation.to());
        Ok(relocation.reversed())
    });
    Performed {
        reversal: operation(reversed, EntryOperation::Relocate),
        unperformed: operation(failed, EntryOperation::Relocate),
        failures,
    }
}

/// What [`execute`] did to several lists and folders, as the report of failures names it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EntriesOperation {
    Deleted,
    Moved,
}

impl Display for EntriesOperation {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Deleted => "deleted",
            Self::Moved => "moved",
        })
    }
}

impl App {
    /// Records that the list at `list` was saved with content of the given `fingerprint`, so
    /// that the next save expects it on disk. Does nothing if the list is not loaded.
    pub fn saved(&mut self, list: &ListPath, fingerprint: Fingerprint) {
        if let Some(Ok(loaded)) = self.workspace.list_mut(list).map(List::content_mut) {
            loaded.set_fingerprint(fingerprint);
        }
    }

    /// Reports that saving a list failed with `error`.
    pub fn save_failed(&mut self, error: &SaveError) {
        let report = error_report(error);
        tracing::warn!(%report, "cannot save list");
        self.show_message(StatusMessage::error(report));
    }

    /// Adds the folder or list `name`, just created in the folder `parent`, to the workspace
    /// and selects it, unfolding the folders that contain it.
    fn entry_created(&mut self, kind: EntryKind, parent: &Path, name: &EntryName) {
        let Some(entry) = self.workspace.insert(kind, parent, name) else {
            tracing::warn!(parent = %parent.display(), %name, "folder of a new entry is gone");
            return;
        };
        let key = EntryKey::from(entry);
        let viewport_height = self.viewport_height(Focus::Lists);
        self.lists.select(&self.workspace, &key, viewport_height);
    }

    /// Renames the folder or list at `path`, just renamed to `name` on disk, in the workspace
    /// and keeps it selected; state that remembers its lists follows them.
    fn entry_renamed(&mut self, path: &Path, name: &EntryName) {
        let Some(entry) = self.workspace.rename(path, name) else {
            tracing::warn!(path = %path.display(), %name, "renamed entry is gone");
            return;
        };
        let key = EntryKey::from(entry);
        self.entry_relocated(path, &key);
    }

    /// Moves the folder or list at `path`, just moved into `folder` on disk, in the workspace
    /// and selects it; state that remembers its lists follows them.
    fn entry_moved(&mut self, path: &Path, folder: &Path) {
        let Some(entry) = self.workspace.move_into(path, folder) else {
            tracing::warn!(
                path = %path.display(),
                folder = %folder.display(),
                "moved entry or its folder is gone"
            );
            return;
        };
        let key = EntryKey::from(entry);
        self.entry_relocated(path, &key);
    }

    /// Follows the folder or list that was at `from` and is now the entry with `key` in every
    /// piece of state that remembers it, and selects it.
    fn entry_relocated(&mut self, from: &Path, key: &EntryKey) {
        self.apply_entry_change(EntryChange::Renamed {
            from,
            to: key.path(),
        });
        let viewport_height = self.viewport_height(Focus::Lists);
        self.lists.select(&self.workspace, key, viewport_height);
    }

    /// Moves the folder or list at `path`, just moved to `to` on disk, in the workspace and
    /// selects it; state that remembers its lists follows them.
    fn entry_relocated_to(&mut self, path: &Path, to: &Path) {
        let Some(entry) = self.workspace.relocate(path, to) else {
            tracing::warn!(
                path = %path.display(),
                to = %to.display(),
                "relocated entry or its folder is gone"
            );
            return;
        };
        let key = EntryKey::from(entry);
        self.entry_relocated(path, &key);
    }

    /// Removes the folder or list at `path`, just moved into the bin, from the workspace; the
    /// selection moves to a neighbour, a list inside it is closed and the undo history of its
    /// documents is kept for its restore.
    fn entry_deleted(&mut self, path: &Path) {
        if self.workspace.remove(path).is_none() {
            tracing::warn!(path = %path.display(), "deleted entry was not in the workspace");
        }
        self.apply_entry_change(EntryChange::Removed(path));
    }

    /// Adds the folders and lists at `paths`, just restored from the bin, to the workspace by
    /// scanning it again, makes `active` the active list again if it is back and selects the
    /// first of the entries.
    fn entries_restored(&mut self, paths: &[PathBuf], active: Option<&ListPath>) {
        self.rescan();
        let viewport_height = self.viewport_height(Focus::Lists);
        if let Some(active) = active.filter(|list| self.workspace.list(list).is_some()) {
            self.lists.open(&self.workspace, active, viewport_height);
        }
        let keys: Vec<EntryKey> = paths
            .iter()
            .rev()
            .filter_map(|path| self.workspace.entry(path).map(EntryKey::from))
            .collect();
        for key in &keys {
            self.lists.select(&self.workspace, key, viewport_height);
        }
        self.refresh_tasks();
    }

    /// Records `reversal`, which reverts a change of lists and folders just made, as one step
    /// of the undo history; `None` if nothing changed.
    fn record_entries(&mut self, reversal: Option<EntryOperation>) {
        if let Some(reversal) = reversal {
            self.history.record(HistoryEntry::Entries(reversal));
        }
    }

    /// Stores what an undo or redo in `direction` of a change of lists and folders
    /// `performed`: the reversal for the opposite step and the part that failed for the next
    /// step in `direction`; then reports the step or its failures.
    fn entries_stepped(&mut self, direction: Direction, performed: Performed) {
        let Performed {
            reversal,
            unperformed,
            failures,
        } = performed;
        if let Some(unperformed) = unperformed {
            self.history
                .keep(direction, HistoryEntry::Entries(unperformed));
        }
        if let Some(reversal) = reversal {
            let summary = entries_summary(&reversal.workspace_paths());
            self.history
                .put_back(direction, HistoryEntry::Entries(reversal));
            if failures.is_empty() {
                self.show_message(StatusMessage::info(format!("{direction}: {summary}")));
                return;
            }
        }
        let reports: Vec<String> = failures.iter().map(|error| error_report(error)).collect();
        let report = format!("cannot {direction}: {}", reports.join("; "));
        tracing::warn!(%report, "cannot step through the history of lists and folders");
        self.show_message(StatusMessage::error(report));
    }

    /// Reports the `failures` of an `operation` on the lists and folders `entries` in one
    /// message: the error itself for a single entry, otherwise how many succeeded and every
    /// error, such as `deleted 298 of 300 lists; 2 failed: ...`.
    fn entries_failed(
        &mut self,
        operation: EntriesOperation,
        entries: &[EntryKey],
        failures: &[EntryError],
    ) {
        let report = match failures {
            [] => return,
            [error] if entries.len() == 1 => error_report(error),
            _ => {
                let reports: Vec<String> =
                    failures.iter().map(|error| error_report(error)).collect();
                format!(
                    "{operation} {} of {} {}; {} failed: {}",
                    entries.len() - failures.len(),
                    entries.len(),
                    entries_noun(entries),
                    failures.len(),
                    reports.join("; ")
                )
            }
        };
        tracing::warn!(%report, "cannot change entries");
        self.show_message(StatusMessage::error(report));
    }

    /// Reports that creating or renaming a list or folder failed with `error`.
    fn entry_failed(&mut self, error: &EntryError) {
        let report = error_report(error);
        tracing::warn!(%report, "cannot change entry");
        self.show_message(StatusMessage::error(report));
    }
}

/// Names the lists and folders at `paths` in a message: the name of a single one, as the lists
/// pane shows it, or their number.
fn entries_summary(paths: &[&Path]) -> String {
    let name = match paths {
        [path] if has_list_extension(path) => path.file_stem(),
        [path] => path.file_name(),
        several => return format!("{} lists and folders", several.len()),
    };
    name.unwrap_or_default().to_string_lossy().into_owned()
}

/// Returns what `entries` are in the plural: `lists`, `folders`, or `entries` for both.
fn entries_noun(entries: &[EntryKey]) -> &'static str {
    let lists = entries
        .iter()
        .filter(|entry| matches!(entry, EntryKey::List(_)))
        .count();
    match lists {
        0 => "folders",
        lists if lists == entries.len() => "lists",
        _ => "entries",
    }
}

/// Describes `error` and all its sources in one line, separated by `: `.
pub(super) fn error_report(error: &dyn Error) -> String {
    iter::successors(error.source(), |&cause| cause.source()).fold(
        error.to_string(),
        |mut report, cause| {
            write!(report, ": {cause}").expect("writing to a string cannot fail");
            report
        },
    )
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::io;
    use std::path::PathBuf;

    use rstest::rstest;
    use tempfile::TempDir;

    use super::*;
    use crate::app::state::tests::{app_in, only_list_path, toggle_first_task};

    #[rstest]
    #[case::lists(&["a.md", "b.md"], "lists")]
    #[case::folders(&["a", "b"], "folders")]
    #[case::both(&["a", "b.md"], "entries")]
    fn entries_are_named_by_their_kind(#[case] paths: &[&str], #[case] expected: &str) {
        let directory = TempDir::new().unwrap();
        for path in paths {
            let absolute = directory.path().join(path);
            if Path::new(path).extension().is_some() {
                fs::write(absolute, "").unwrap();
            } else {
                fs::create_dir(absolute).unwrap();
            }
        }
        let app = app_in(directory.path());
        let entries: Vec<EntryKey> = paths
            .iter()
            .map(|path| EntryKey::from(app.workspace().entry(Path::new(path)).unwrap()))
            .collect();

        let system_under_test = entries_noun(&entries);

        assert_eq!(system_under_test, expected);
    }

    #[test]
    fn moving_into_a_folder_that_vanished_reports_the_error() {
        let directory = TempDir::new().unwrap();
        fs::write(directory.path().join("a.md"), "").unwrap();
        let mut app = app_in(directory.path());
        let entries = vec![EntryKey::List(only_list_path(&app))];
        execute(
            &mut app,
            Effect::MoveEntries {
                entries,
                folder: PathBuf::from("gone"),
            },
        );

        let system_under_test = app.status_message().map(StatusMessage::text);

        assert!(system_under_test.is_some_and(|text| text.starts_with("cannot change")));
        assert!(directory.path().join("a.md").exists());
    }

    #[test]
    fn save_writes_the_document_and_remembers_its_fingerprint() {
        let directory = TempDir::new().unwrap();
        let file = directory.path().join("groceries.md");
        fs::write(&file, "- [ ] Buy milk\n").unwrap();
        let mut app = app_in(directory.path());
        let list = only_list_path(&app);
        let expected = "- [x] Buy milk\n";
        for effect in app.edit_list(&list, toggle_first_task) {
            execute(&mut app, effect);
        }

        let system_under_test = app.workspace().list(&list).unwrap().content().unwrap();

        assert_eq!(fs::read_to_string(&file).unwrap(), expected);
        assert_eq!(
            system_under_test.fingerprint(),
            Fingerprint::of(expected.as_bytes())
        );
        assert_eq!(app.status_message(), None);
    }

    #[test]
    fn failed_save_becomes_an_error_message() {
        let directory = TempDir::new().unwrap();
        let file = directory.path().join("groceries.md");
        fs::write(&file, "- [ ] Buy milk\n").unwrap();
        let mut app = app_in(directory.path());
        let list = only_list_path(&app);
        fs::remove_file(&file).unwrap();
        let expected =
            StatusMessage::error(format!("list file {} no longer exists", file.display()));
        execute(&mut app, Effect::Save(list));

        let system_under_test = app.status_message();

        assert_eq!(system_under_test, Some(&expected));
        assert!(!file.exists());
    }

    #[test]
    fn save_of_a_list_that_is_not_loaded_does_nothing() {
        let directory = TempDir::new().unwrap();
        let file = directory.path().join("binary.md");
        fs::write(&file, [0xff]).unwrap();
        let mut app = app_in(directory.path());
        let list = only_list_path(&app);
        execute(&mut app, Effect::Save(list));

        let system_under_test = app.status_message();

        assert_eq!(system_under_test, None);
        assert_eq!(fs::read(&file).unwrap(), [0xff]);
    }

    #[test]
    fn save_failure_reports_the_cause() {
        let directory = TempDir::new().unwrap();
        let mut app = app_in(directory.path());
        let error = SaveError::Write {
            path: PathBuf::from("/todo/groceries.md"),
            source: io::Error::other("disk full"),
        };
        let expected = StatusMessage::error("cannot write list file /todo/groceries.md: disk full");
        app.save_failed(&error);

        let system_under_test = app.status_message();

        assert_eq!(system_under_test, Some(&expected));
    }
}

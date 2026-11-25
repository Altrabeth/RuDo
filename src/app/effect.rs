//! The side effects the application asks for, and [`execute`], which performs them.
//!
//! The runtime and the tests share [`execute`], so the tests exercise the same file system
//! code as a running RuDo.

use std::collections::HashSet;
use std::error::Error;
use std::fmt::{self, Display, Formatter, Write};
use std::iter;
use std::num::NonZeroUsize;
use std::path::{Path, PathBuf};

use crate::app::bin::emptied_message;
use crate::app::history::{Direction, EntryOperation, HistoryEntry, Relocation};
use crate::app::lists::{EntryChange, EntryKey, counted};
use crate::app::state::{App, Focus};
use crate::app::status::{Severity, StatusMessage};
use crate::storage::bin::{
    EmptiedBin, TrashedEntry, delete_from_bin, empty_bin, move_to_bin, restore,
};
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
    /// Move these lists and folders from the bin to the paths they name, as one new step of
    /// the undo history, as the bin view asks for it.
    RestoreEntries(Vec<TrashedEntry>),
    /// Delete the lists and folders at these paths, relative to the bin, permanently; this
    /// cannot be undone.
    DeleteFromBin(Vec<PathBuf>),
    /// Delete everything in the bin permanently; this cannot be undone.
    EmptyBin,
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
        Effect::RestoreEntries(entries) => restore_from_bin(app, entries),
        Effect::DeleteFromBin(locations) => delete_permanently(app, &locations),
        Effect::EmptyBin => {
            let workspace = app.workspace();
            let emptied = empty_bin(workspace.root(), workspace.bin());
            app.bin_emptied(&emptied);
        }
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

/// Moves the lists and folders `entries` from the bin to the paths they name, records their
/// move back into the bin as one undo step and reports the result in one message.
fn restore_from_bin(app: &mut App, entries: Vec<TrashedEntry>) {
    let requested = entries.len();
    let (restored, skipped, failures) = restore_each(app, entries, None);
    let reversal = operation(restored, EntryOperation::Trash);
    if let Some(reversal) = &reversal
        && failures.is_empty()
    {
        let summary = entries_summary(&reversal.workspace_paths());
        app.show_message(StatusMessage::info(format!("restored {summary}")));
    }
    app.record_entries(reversal);
    app.restore_failed(requested, &skipped, &failures);
}

/// Deletes the lists and folders at `locations`, relative to the bin, permanently and reports
/// the result in one message.
fn delete_permanently(app: &mut App, locations: &[PathBuf]) {
    let (deleted, _, failures) = attempt(locations.iter().collect(), |location| {
        let workspace = app.workspace();
        delete_from_bin(workspace.root(), workspace.bin(), location).map(|()| location.as_path())
    });
    app.deleted_from_bin(&deleted, &failures);
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
        app.entry_trashed(trashed.clone());
        Ok(trashed)
    });
    if !trashed.is_empty() {
        app.bin_changed();
    }
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
    let (restored, failed, failures) = restore_each(app, entries, active.as_ref());
    Performed {
        reversal: operation(restored, EntryOperation::Trash),
        unperformed: operation(failed, |entries| EntryOperation::Restore {
            entries,
            active,
        }),
        failures,
    }
}

/// Moves the lists and folders `entries` from the bin back to the paths they name, adds them to
/// the workspace and makes `active` the active list again; returns the restored paths, the
/// entries that stayed in the bin and why.
fn restore_each(
    app: &mut App,
    entries: Vec<TrashedEntry>,
    active: Option<&ListPath>,
) -> (Vec<PathBuf>, Vec<TrashedEntry>, Vec<EntryError>) {
    let (restored, failed, failures) = attempt(entries, |entry| {
        let workspace = app.workspace();
        restore(workspace.root(), workspace.bin(), entry)?;
        app.workspace.remove_bin_entry(entry.location());
        Ok(entry.original().to_path_buf())
    });
    if !restored.is_empty() {
        app.entries_restored(&restored, active);
        app.bin_changed();
    }
    (restored, failed, failures)
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

    /// Moves the folder or list just moved into the bin as `trashed` from the tree into the
    /// bin of the workspace; the selection moves to a neighbour, a list inside it is closed and
    /// the undo history of its documents is kept for its restore.
    fn entry_trashed(&mut self, trashed: TrashedEntry) {
        let original = trashed.original().to_path_buf();
        if !self.workspace.trash(trashed) {
            tracing::warn!(path = %original.display(), "deleted entry was not in the workspace");
        }
        self.apply_entry_change(EntryChange::Removed(&original));
    }

    /// Removes the entries of the bin at the locations `deleted`, just deleted permanently,
    /// from the bin and from the undo history, which can no longer restore them, and reports
    /// them and the `failures` of the others in one message.
    fn deleted_from_bin(&mut self, deleted: &[&Path], failures: &[EntryError]) {
        let names: Vec<String> = deleted
            .iter()
            .filter_map(|location| self.workspace.remove_bin_entry(location))
            .map(|entry| entry.entry().name().to_owned())
            .collect();
        let gone: HashSet<&Path> = deleted.iter().copied().collect();
        self.history.forget_trashed(|entry| {
            entry
                .location()
                .ancestors()
                .any(|location| gone.contains(location))
        });
        if !failures.is_empty() {
            // Part of a folder may be gone, so the bin is read again.
            self.workspace.rescan_bin();
        }
        let message = deleted_message(&names, failures);
        if message.severity() == Severity::Error {
            tracing::warn!(report = message.text(), "cannot delete from the bin");
        }
        self.show_message(message);
        self.bin_changed();
    }

    /// Reads the bin again after it was `emptied`, forgets the entries that are gone in the
    /// undo history and reports the entries that could not be deleted in one message, such as
    /// `emptied 298 of 300 entries of the bin; 2 failed: ...`.
    fn bin_emptied(&mut self, emptied: &EmptiedBin) {
        self.workspace.rescan_bin();
        let remaining = self.workspace.bin_entries();
        self.history.forget_trashed(|trashed| {
            !remaining
                .iter()
                .any(|entry| trashed.location().starts_with(entry.location()))
        });
        let message = emptied_message(emptied);
        if message.severity() == Severity::Error {
            tracing::warn!(report = message.text(), "cannot empty the bin");
        }
        self.show_message(message);
        self.bin_changed();
    }

    /// Reports why restoring the `skipped` of `requested` entries from the bin failed with
    /// `failures`, in one message, and keeps the skipped entries of a restore of several
    /// selected in visual mode.
    ///
    /// A single entry with a taken original path says how to free it; several name the taken
    /// paths: `restored 8 of 10; 2 paths are taken: Home/Groceries.md, Work`.
    fn restore_failed(
        &mut self,
        requested: usize,
        skipped: &[TrashedEntry],
        failures: &[EntryError],
    ) {
        let report = match failures {
            [] => return,
            [error] if requested == 1 => {
                if is_taken(error) {
                    format!(
                        "cannot restore: {}; rename or move it first",
                        error_report(error)
                    )
                } else {
                    format!("cannot restore: {}", error_report(error))
                }
            }
            several => restored_report(requested, skipped, several),
        };
        tracing::warn!(%report, "cannot restore from the bin");
        self.show_message(StatusMessage::error(report));
        if requested > 1 {
            let locations: Vec<PathBuf> = skipped
                .iter()
                .map(|entry| entry.location().to_path_buf())
                .collect();
            self.select_in_bin(&locations);
        }
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

/// Returns whether `error` says that another entry took the path an entry was restored to.
const fn is_taken(error: &EntryError) -> bool {
    match error {
        EntryError::AlreadyExists { .. } => true,
        EntryError::Io { .. }
        | EntryError::IntoItself { .. }
        | EntryError::NotEmpty { .. }
        | EntryError::InvalidName(_) => false,
    }
}

/// Reports a restore of `requested` entries that left the `skipped` entries in the bin because
/// of `failures`: how many were restored, the original paths that are taken and every other
/// error, such as `restored 8 of 10; 2 paths are taken: Home/Groceries.md, Work`.
fn restored_report(requested: usize, skipped: &[TrashedEntry], failures: &[EntryError]) -> String {
    let (taken, others): (Vec<_>, Vec<_>) = skipped
        .iter()
        .zip(failures)
        .partition(|(_, error)| is_taken(error));
    let mut report = format!("restored {} of {requested}", requested - failures.len());
    if !taken.is_empty() {
        let paths: Vec<String> = taken
            .iter()
            .map(|(entry, _)| entry.original().display().to_string())
            .collect();
        let verb = if taken.len() == 1 { "is" } else { "are" };
        let count = counted(taken.len(), "path", "paths");
        write!(report, "; {count} {verb} taken: {}", paths.join(", "))
            .expect("writing to a string cannot fail");
    }
    if !others.is_empty() {
        let reports: Vec<String> = others
            .iter()
            .map(|(_, error)| error_report(error))
            .collect();
        write!(report, "; {} failed: {}", others.len(), reports.join("; "))
            .expect("writing to a string cannot fail");
    }
    report
}

/// Reports a permanent deletion from the bin: `deleted Work permanently` for the single entry
/// named in `names`, their number for several, or the `failures` of the others in one error,
/// such as `deleted 10 of 12 entries permanently; 2 failed: ...`.
fn deleted_message(names: &[String], failures: &[EntryError]) -> StatusMessage {
    let requested = names.len() + failures.len();
    match (names, failures) {
        ([name], []) => StatusMessage::info(format!("deleted {name} permanently")),
        (_, []) => StatusMessage::info(format!("deleted {requested} entries permanently")),
        ([], [error]) => StatusMessage::error(error_report(error)),
        (_, _) => {
            let reports: Vec<String> = failures.iter().map(|error| error_report(error)).collect();
            StatusMessage::error(format!(
                "deleted {} of {requested} entries permanently; {} failed: {}",
                names.len(),
                failures.len(),
                reports.join("; ")
            ))
        }
    }
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

    fn taken(path: &str) -> EntryError {
        EntryError::AlreadyExists {
            path: PathBuf::from(path),
        }
    }

    fn failed(path: &str) -> EntryError {
        EntryError::Io {
            path: PathBuf::from(path),
            source: io::Error::other("denied"),
        }
    }

    /// Moves the lists `names`, created in a new workspace `directory`, into its bin.
    fn trashed(directory: &TempDir, names: &[&str]) -> Vec<TrashedEntry> {
        let bin = crate::storage::bin::BinPath::default();
        names
            .iter()
            .map(|name| {
                fs::write(directory.path().join(name), "").unwrap();
                move_to_bin(directory.path(), &bin, Path::new(name)).unwrap()
            })
            .collect()
    }

    /// Whether a skipped entry of a restore found its path taken or failed otherwise.
    #[derive(Debug, Clone, Copy)]
    enum Skip {
        Taken,
        Failed,
    }

    #[rstest]
    #[case::one_taken(&[("a.md", Skip::Taken)], 3, "restored 2 of 3; 1 path is taken: a.md")]
    #[case::taken(
        &[("a.md", Skip::Taken), ("b.md", Skip::Taken)],
        2,
        "restored 0 of 2; 2 paths are taken: a.md, b.md"
    )]
    #[case::other_errors(
        &[("a.md", Skip::Taken), ("x.md", Skip::Failed)],
        4,
        "restored 2 of 4; 1 path is taken: a.md; 1 failed: cannot change x.md: denied"
    )]
    #[case::only_other_errors(
        &[("x.md", Skip::Failed)],
        2,
        "restored 1 of 2; 1 failed: cannot change x.md: denied"
    )]
    fn restored_report_names_the_taken_paths_and_the_other_errors(
        #[case] skipped: &[(&str, Skip)],
        #[case] requested: usize,
        #[case] expected: &str,
    ) {
        let directory = TempDir::new().unwrap();
        let names: Vec<&str> = skipped.iter().map(|&(name, _)| name).collect();
        let entries = trashed(&directory, &names);
        let failures: Vec<EntryError> = skipped
            .iter()
            .map(|&(name, skip)| match skip {
                Skip::Taken => taken(name),
                Skip::Failed => failed(name),
            })
            .collect();

        let system_under_test = restored_report(requested, &entries, &failures);

        assert_eq!(system_under_test, expected);
    }

    #[rstest]
    #[case::one(&["Work"], &[], StatusMessage::info("deleted Work permanently"))]
    #[case::several(&["Work", "a"], &[], StatusMessage::info("deleted 2 entries permanently"))]
    #[case::one_failed(&[], &["x"], StatusMessage::error("cannot change x: denied"))]
    #[case::some_failed(
        &["Work"],
        &["x", "y"],
        StatusMessage::error(
            "deleted 1 of 3 entries permanently; 2 failed: cannot change x: denied; \
             cannot change y: denied"
        )
    )]
    fn deleted_message_reports_the_deleted_entries_and_every_failure(
        #[case] names: &[&str],
        #[case] failures: &[&str],
        #[case] expected: StatusMessage,
    ) {
        let names: Vec<String> = names.iter().map(|name| (*name).to_owned()).collect();
        let failures: Vec<EntryError> = failures.iter().map(|path| failed(path)).collect();

        let system_under_test = deleted_message(&names, &failures);

        assert_eq!(system_under_test, expected);
    }
}

//! The side effects the application asks for, and [`execute`], which performs them.
//!
//! The runtime and the tests share [`execute`], so the tests exercise the same file system
//! code as a running RuDo.

use std::error::Error;
use std::fmt::{self, Display, Formatter, Write};
use std::iter;
use std::num::NonZeroUsize;
use std::path::{Path, PathBuf};

use crate::app::lists::{EntryChange, EntryKey};
use crate::app::state::{App, Focus};
use crate::app::status::StatusMessage;
use crate::storage::entries::{
    EntryError, EntryName, create_folder, create_list, delete_folder, delete_list, move_into,
    rename,
};
use crate::storage::fingerprint::Fingerprint;
use crate::storage::save::{SaveError, save};
use crate::storage::workspace::{EntryKind, List, ListPath};

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
    /// Delete these list files and empty folders; folders are never deleted recursively.
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
        Ok(_) => app.entry_created(kind, parent, name),
        Err(error) => app.entry_failed(&error),
    }
}

/// Renames the list file or folder at `path` to `name` on disk.
fn rename_entry(app: &mut App, path: &Path, name: &EntryName) {
    match rename(&app.workspace().root().join(path), name) {
        Ok(_) => app.entry_renamed(path, name),
        Err(error) => app.entry_failed(&error),
    }
}

/// Deletes the list files and empty folders `entries` on disk, removes the deleted ones from
/// the workspace and reports the failures in one message.
fn delete_entries(app: &mut App, entries: &[EntryKey]) {
    let failures: Vec<EntryError> = entries
        .iter()
        .filter_map(|entry| match delete_entry(app.workspace().root(), entry) {
            Ok(()) => {
                app.entry_deleted(entry.path());
                None
            }
            Err(error) => Some(error),
        })
        .collect();
    app.entries_failed(EntriesOperation::Deleted, entries, &failures);
}

/// Deletes the list file or empty folder `entry` of the workspace in `root` on disk.
fn delete_entry(root: &Path, entry: &EntryKey) -> Result<(), EntryError> {
    let absolute = root.join(entry.path());
    match entry {
        EntryKey::Folder(_) => delete_folder(&absolute),
        EntryKey::List(_) => delete_list(&absolute),
    }
}

/// Moves the list files and folders `entries` into `folder` on disk, follows the moved ones in
/// the workspace and reports the failures in one message.
fn move_entries(app: &mut App, entries: &[EntryKey], folder: &Path) {
    let directory = app.workspace().root().join(folder);
    let failures: Vec<EntryError> = entries
        .iter()
        .filter_map(|entry| {
            match move_into(&app.workspace().root().join(entry.path()), &directory) {
                Ok(_) => {
                    app.entry_moved(entry.path(), folder);
                    None
                }
                Err(error) => Some(error),
            }
        })
        .collect();
    app.entries_failed(EntriesOperation::Moved, entries, &failures);
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

    /// Removes the folder or list at `path`, just deleted on disk, from the workspace; the
    /// selection moves to a neighbour and state that remembers the list forgets it.
    fn entry_deleted(&mut self, path: &Path) {
        if self.workspace.remove(path).is_none() {
            tracing::warn!(path = %path.display(), "deleted entry was not in the workspace");
        }
        self.apply_entry_change(EntryChange::Removed(path));
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

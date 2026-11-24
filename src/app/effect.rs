//! The side effects the application asks for, and [`execute`], which performs them.
//!
//! The runtime and the tests share [`execute`], so the tests exercise the same file system
//! code as a running RuDo.

use std::error::Error;
use std::fmt::Write;
use std::iter;
use std::path::{Path, PathBuf};

use crate::app::lists::EntryKey;
use crate::app::state::{App, Focus};
use crate::app::status::StatusMessage;
use crate::storage::entries::{EntryError, EntryName, create_folder, create_list};
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
}

/// Performs `effect` and reports its result to `app`.
///
/// Failures never abort: they become a [`StatusMessage`] on `app`.
pub fn execute(app: &mut App, effect: Effect) {
    match effect {
        Effect::Save(list) => save_list(app, &list),
        Effect::CreateEntry { kind, parent, name } => create_entry(app, kind, &parent, &name),
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
        Err(error) => app.create_failed(&error),
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

    /// Reports that creating a list or folder failed with `error`.
    fn create_failed(&mut self, error: &EntryError) {
        let report = error_report(error);
        tracing::warn!(%report, "cannot create entry");
        self.show_message(StatusMessage::error(report));
    }
}

/// Describes `error` and all its sources in one line, separated by `: `.
fn error_report(error: &dyn Error) -> String {
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

    use tempfile::TempDir;

    use super::*;
    use crate::app::state::tests::{app_in, only_list_path, toggle_first_task};

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

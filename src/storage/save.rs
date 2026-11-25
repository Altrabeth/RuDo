//! Atomic saving of list files that refuses to overwrite changes made by other programs.
//!
//! [`save`] compares the file on disk with the [`Fingerprint`] of the content RuDo loaded and
//! only then replaces it. The replacement is atomic: the new content goes into a temporary file
//! in the same directory, is flushed to disk and renamed over the target, so a crash leaves
//! either the old or the new file, never a mix. The temporary file's name starts with `.tmp`,
//! so the workspace scanner and the watcher ignore it as a hidden file.
//!
//! Windows refuses the rename for a moment while another program, such as a virus scanner or
//! a search indexer, holds the list open without allowing it to be deleted; the rename is then
//! retried a few times before the save fails.

use std::fs::{self, File, Permissions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::thread;
use std::time::Duration;

use tempfile::NamedTempFile;

use crate::markdown::document::TodoDocument;
use crate::platform::Platform;
use crate::storage::fingerprint::Fingerprint;

/// How often the rename over a list is tried before the save fails.
const RENAME_ATTEMPTS: usize = 5;

/// How long the save waits before it tries a refused rename again.
const RENAME_RETRY_DELAY: Duration = Duration::from_millis(20);

/// Errors that occur while saving a list file.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum SaveError {
    /// The file was deleted since it was loaded.
    #[error("list file {} no longer exists", path.display())]
    Missing {
        /// File that was to be saved.
        path: PathBuf,
    },
    /// Another program changed the file since it was loaded; it was left untouched.
    #[error("list file {} was modified by another program", path.display())]
    ModifiedExternally {
        /// File that was to be saved.
        path: PathBuf,
    },
    /// The current content of the file could not be read.
    #[error("cannot read list file {}", path.display())]
    Read {
        /// File that could not be read.
        path: PathBuf,
        /// Underlying I/O error.
        #[source]
        source: io::Error,
    },
    /// The new content could not be written.
    #[error("cannot write list file {}", path.display())]
    Write {
        /// File that could not be written.
        path: PathBuf,
        /// Underlying I/O error.
        #[source]
        source: io::Error,
    },
}

/// Saves `document` to the list file at `path` unless another program changed the file.
///
/// `expected` is the fingerprint of the content RuDo last loaded from or saved to `path`. The
/// file is only replaced if its current content still has that fingerprint. Symbolic links are
/// resolved first, so a symlinked list stays a symlink and its target receives the content.
/// The file keeps its permissions, on Windows its read-only attribute.
///
/// Another program can still change the file in the short window between the comparison and
/// the rename; that change is then lost. Closing the window would require file locks, which
/// editors such as Obsidian do not honour, so the window is accepted.
///
/// Returns the fingerprint of the saved content, to be passed as `expected` to the next save.
///
/// # Errors
///
/// Returns [`SaveError::Missing`] if the file no longer exists and
/// [`SaveError::ModifiedExternally`] if its content no longer matches `expected`; the disk is
/// left untouched in both cases. Returns [`SaveError::Read`] if the current content cannot be
/// read and [`SaveError::Write`] if the new content cannot be written, in which case the
/// original file is unchanged and no temporary file remains.
///
/// # Examples
///
/// ```
/// use rudo::markdown::document::TodoDocument;
/// use rudo::storage::fingerprint::Fingerprint;
/// use rudo::storage::save::save;
///
/// let directory = tempfile::TempDir::new()?;
/// let path = directory.path().join("groceries.md");
/// std::fs::write(&path, "- [ ] Buy milk\n")?;
/// let loaded = Fingerprint::of(b"- [ ] Buy milk\n");
///
/// let saved = save(&path, &TodoDocument::parse("- [x] Buy milk\n"), loaded)?;
///
/// assert_eq!(std::fs::read_to_string(&path)?, "- [x] Buy milk\n");
/// assert_eq!(saved, Fingerprint::of(b"- [x] Buy milk\n"));
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
pub fn save(
    path: &Path,
    document: &TodoDocument,
    expected: Fingerprint,
) -> Result<Fingerprint, SaveError> {
    let target = fs::canonicalize(path).map_err(|error| read_error(path, error))?;
    let (current, permissions) =
        read_with_permissions(&target).map_err(|error| read_error(path, error))?;
    if Fingerprint::of(&current) != expected {
        return Err(SaveError::ModifiedExternally {
            path: path.to_path_buf(),
        });
    }
    let content = document.to_string();
    replace_atomically(&target, content.as_bytes(), permissions).map_err(|source| {
        SaveError::Write {
            path: path.to_path_buf(),
            source,
        }
    })?;
    Ok(Fingerprint::of(content.as_bytes()))
}

/// Classifies an error that occurred while looking at the current file: a file that is gone
/// was deleted since it was loaded.
fn read_error(path: &Path, error: io::Error) -> SaveError {
    let path = path.to_path_buf();
    match error.kind() {
        io::ErrorKind::NotFound => SaveError::Missing { path },
        _ => SaveError::Read {
            path,
            source: error,
        },
    }
}

/// Reads the content and the permissions of `path` through one open file handle.
fn read_with_permissions(path: &Path) -> io::Result<(Vec<u8>, Permissions)> {
    let mut file = File::open(path)?;
    let permissions = file.metadata()?.permissions();
    let mut content = Vec::new();
    file.read_to_end(&mut content)?;
    Ok((content, permissions))
}

/// Replaces the file `target` with `content` so that readers see either the old or the new
/// content, never a partial write.
///
/// The temporary file must live next to `target` because a rename across file systems fails.
/// It is deleted when it is dropped, so no temporary file remains after an error.
fn replace_atomically(target: &Path, content: &[u8], permissions: Permissions) -> io::Result<()> {
    let directory = target
        .parent()
        .expect("a canonical path to a file always has a parent directory");
    let mut temporary = NamedTempFile::new_in(directory)?;
    temporary.write_all(content)?;
    temporary.as_file().sync_all()?;
    persist_keeping_permissions(temporary, target, permissions)?;
    // Windows cannot open a directory for flushing; its rename is durable once it returns.
    #[cfg(unix)]
    sync_directory(directory)?;
    Ok(())
}

/// Renames `temporary` over `target` and gives it `permissions`, which it receives before the
/// rename, so that the new content is never visible with other permissions.
#[cfg(not(windows))]
fn persist_keeping_permissions(
    temporary: NamedTempFile,
    target: &Path,
    permissions: Permissions,
) -> io::Result<()> {
    temporary.as_file().set_permissions(permissions)?;
    persist_with_retries(temporary, target, |error| {
        is_transient_rename_error(Platform::current(), error)
    })
}

/// Renames `temporary` over `target` and gives it `permissions`, which on Windows are the
/// file attributes.
///
/// Windows refuses to rename over a read-only file, and the rename resets the attributes of
/// the temporary file, so a read-only target loses the attribute for the moment of the rename
/// and gets it back afterwards, also when the rename fails.
#[cfg(windows)]
fn persist_keeping_permissions(
    temporary: NamedTempFile,
    target: &Path,
    permissions: Permissions,
) -> io::Result<()> {
    let persist = |temporary| {
        persist_with_retries(temporary, target, |error| {
            is_transient_rename_error(Platform::current(), error)
        })
    };
    if !permissions.readonly() {
        return persist(temporary);
    }
    let mut writable = permissions.clone();
    #[expect(
        clippy::permissions_set_readonly_false,
        reason = "on Windows this clears the read-only attribute and grants nothing to others"
    )]
    writable.set_readonly(false);
    fs::set_permissions(target, writable)?;
    let persisted = persist(temporary);
    let restored = fs::set_permissions(target, permissions);
    persisted.and(restored)
}

/// Renames `temporary` over `target`, trying again after [`RENAME_RETRY_DELAY`] while
/// `is_transient` holds for the error, at most [`RENAME_ATTEMPTS`] times in all. The temporary
/// file is deleted when the last attempt fails.
fn persist_with_retries(
    mut temporary: NamedTempFile,
    target: &Path,
    is_transient: impl Fn(&io::Error) -> bool,
) -> io::Result<()> {
    for _ in 1..RENAME_ATTEMPTS {
        match temporary.persist(target) {
            Ok(_) => return Ok(()),
            Err(error) if is_transient(&error.error) => {
                tracing::debug!(error = %error.error, target = %target.display(), "rename refused, retrying");
                temporary = error.file;
                thread::sleep(RENAME_RETRY_DELAY);
            }
            Err(error) => return Err(error.error),
        }
    }
    temporary
        .persist(target)
        .map(drop)
        .map_err(|error| error.error)
}

/// Whether the rename over a list may succeed if it is tried again on `platform`.
///
/// Windows refuses it with "access denied", a sharing violation or a lock violation while
/// another program has the file open without allowing it to be deleted, which virus scanners,
/// search indexers and sync clients do for a moment. Unix systems rename regardless of open
/// files, so a refusal there is final.
fn is_transient_rename_error(platform: Platform, error: &io::Error) -> bool {
    /// `ERROR_ACCESS_DENIED`, `ERROR_SHARING_VIOLATION` and `ERROR_LOCK_VIOLATION`.
    const WINDOWS_ERRORS: [i32; 3] = [5, 32, 33];
    match platform {
        Platform::Windows => error
            .raw_os_error()
            .is_some_and(|code| WINDOWS_ERRORS.contains(&code)),
        Platform::Linux | Platform::MacOs | Platform::OtherUnix => false,
    }
}

/// Flushes the directory entry written by a rename, so that it survives a crash.
#[cfg(unix)]
fn sync_directory(directory: &Path) -> io::Result<()> {
    File::open(directory)?.sync_all()
}

#[cfg(test)]
mod tests {
    use std::fs;

    use rstest::rstest;
    use strum::IntoEnumIterator;
    use tempfile::TempDir;

    use super::*;

    const ORIGINAL: &str = "- [ ] Buy milk\n";
    const EDITED: &str = "- [x] Buy milk\n- [ ] Call mom\n";
    const LIST_NAME: &str = "groceries.md";

    /// Creates a workspace holding one list file with `content`.
    fn workspace_with_list(content: &str) -> (TempDir, PathBuf) {
        let directory = TempDir::new().unwrap();
        let path = directory.path().join(LIST_NAME);
        fs::write(&path, content).unwrap();
        (directory, path)
    }

    /// Names of all entries in `directory`, sorted.
    fn entry_names(directory: &Path) -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(directory)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().into_string().unwrap())
            .collect();
        names.sort_unstable();
        names
    }

    #[test]
    fn save_writes_the_rendered_document() {
        let (_directory, path) = workspace_with_list(ORIGINAL);
        let document = TodoDocument::parse(EDITED);
        let expected = Fingerprint::of(ORIGINAL.as_bytes());

        let system_under_test = save(&path, &document, expected);

        assert!(system_under_test.is_ok());
        assert_eq!(fs::read_to_string(&path).unwrap(), EDITED);
    }

    #[test]
    fn save_returns_the_fingerprint_of_the_new_content() {
        let (_directory, path) = workspace_with_list(ORIGINAL);
        let document = TodoDocument::parse(EDITED);
        let expected = Fingerprint::of(ORIGINAL.as_bytes());
        let new_fingerprint = Fingerprint::of(EDITED.as_bytes());

        let system_under_test = save(&path, &document, expected).unwrap();

        assert_eq!(system_under_test, new_fingerprint);
    }

    #[test]
    fn saves_in_a_row_chain_their_fingerprints() {
        let (_directory, path) = workspace_with_list(ORIGINAL);
        let first_document = TodoDocument::parse(EDITED);
        let second_document = TodoDocument::parse(ORIGINAL);
        let first_fingerprint =
            save(&path, &first_document, Fingerprint::of(ORIGINAL.as_bytes())).unwrap();

        let system_under_test = save(&path, &second_document, first_fingerprint);

        assert!(system_under_test.is_ok());
        assert_eq!(fs::read_to_string(&path).unwrap(), ORIGINAL);
    }

    #[test]
    fn save_refuses_to_overwrite_an_external_modification() {
        let external = "- [ ] Buy milk\n- [ ] Added in Obsidian\n";
        let (_directory, path) = workspace_with_list(external);
        let document = TodoDocument::parse(EDITED);
        let expected = Fingerprint::of(ORIGINAL.as_bytes());

        let system_under_test = save(&path, &document, expected);

        assert!(matches!(
            system_under_test,
            Err(SaveError::ModifiedExternally { path: ref error_path }) if *error_path == path
        ));
        assert_eq!(fs::read_to_string(&path).unwrap(), external);
    }

    #[test]
    fn save_reports_a_deleted_file_as_missing_and_does_not_recreate_it() {
        let (_directory, path) = workspace_with_list(ORIGINAL);
        fs::remove_file(&path).unwrap();
        let document = TodoDocument::parse(EDITED);
        let expected = Fingerprint::of(ORIGINAL.as_bytes());

        let system_under_test = save(&path, &document, expected);

        assert!(matches!(
            system_under_test,
            Err(SaveError::Missing { path: ref error_path }) if *error_path == path
        ));
        assert!(!path.exists());
    }

    #[test]
    fn save_reports_an_unreadable_target_as_a_read_error() {
        let (directory, _path) = workspace_with_list(ORIGINAL);
        let document = TodoDocument::parse(EDITED);
        let expected = Fingerprint::of(ORIGINAL.as_bytes());

        let system_under_test = save(directory.path(), &document, expected);

        assert!(matches!(system_under_test, Err(SaveError::Read { .. })));
    }

    #[test]
    fn save_leaves_no_temporary_file_after_success() {
        let (directory, path) = workspace_with_list(ORIGINAL);
        let document = TodoDocument::parse(EDITED);
        let expected = Fingerprint::of(ORIGINAL.as_bytes());
        let expected_entries = vec![LIST_NAME.to_owned()];
        save(&path, &document, expected).unwrap();

        let system_under_test = entry_names(directory.path());

        assert_eq!(system_under_test, expected_entries);
    }

    #[test]
    fn save_leaves_no_temporary_file_after_failure() {
        let (directory, path) = workspace_with_list("- [ ] changed elsewhere\n");
        let document = TodoDocument::parse(EDITED);
        let expected = Fingerprint::of(ORIGINAL.as_bytes());
        let expected_entries = vec![LIST_NAME.to_owned()];
        save(&path, &document, expected).unwrap_err();

        let system_under_test = entry_names(directory.path());

        assert_eq!(system_under_test, expected_entries);
    }

    #[rstest]
    #[case::access_denied(5, true)]
    #[case::sharing_violation(32, true)]
    #[case::lock_violation(33, true)]
    #[case::file_not_found(2, false)]
    #[case::disk_full(112, false)]
    fn windows_retries_a_rename_refused_by_an_open_file(#[case] code: i32, #[case] expected: bool) {
        let error = io::Error::from_raw_os_error(code);

        let system_under_test = is_transient_rename_error(Platform::Windows, &error);

        assert_eq!(system_under_test, expected);
    }

    #[test]
    fn unix_platforms_never_retry_a_rename() {
        let errors = [5, 13, 16, 32, 33].map(io::Error::from_raw_os_error);

        let system_under_test: Vec<bool> = Platform::iter()
            .filter(|platform| *platform != Platform::Windows)
            .flat_map(|platform| {
                errors
                    .iter()
                    .map(move |error| is_transient_rename_error(platform, error))
            })
            .collect();

        assert!(system_under_test.iter().all(|retried| !retried));
    }

    #[test]
    fn retried_rename_gives_up_and_removes_the_temporary_file() {
        let directory = TempDir::new().unwrap();
        let target = directory.path().join("occupied");
        fs::create_dir(&target).unwrap();
        fs::write(target.join("keep.md"), "").unwrap();
        let temporary = NamedTempFile::new_in(directory.path()).unwrap();
        let attempts = std::cell::Cell::new(0);
        let expected_entries = vec!["occupied".to_owned()];

        let system_under_test = persist_with_retries(temporary, &target, |_| {
            attempts.set(attempts.get() + 1);
            true
        });

        assert!(system_under_test.is_err());
        assert_eq!(attempts.get(), RENAME_ATTEMPTS - 1);
        assert_eq!(entry_names(directory.path()), expected_entries);
    }

    #[test]
    fn rename_is_not_retried_after_a_final_error() {
        let directory = TempDir::new().unwrap();
        let target = directory.path().join("occupied");
        fs::create_dir(&target).unwrap();
        fs::write(target.join("keep.md"), "").unwrap();
        let temporary = NamedTempFile::new_in(directory.path()).unwrap();
        let attempts = std::cell::Cell::new(0);

        let system_under_test = persist_with_retries(temporary, &target, |_| {
            attempts.set(attempts.get() + 1);
            false
        });

        assert!(system_under_test.is_err());
        assert_eq!(attempts.get(), 1);
    }

    #[cfg(windows)]
    #[test]
    fn save_keeps_the_read_only_attribute_on_windows() {
        let (_directory, path) = workspace_with_list(ORIGINAL);
        let mut read_only = fs::metadata(&path).unwrap().permissions();
        read_only.set_readonly(true);
        fs::set_permissions(&path, read_only).unwrap();
        let document = TodoDocument::parse(EDITED);
        let expected = Fingerprint::of(ORIGINAL.as_bytes());

        let system_under_test = save(&path, &document, expected);

        assert!(system_under_test.is_ok());
        assert_eq!(fs::read_to_string(&path).unwrap(), EDITED);
        assert!(fs::metadata(&path).unwrap().permissions().readonly());
    }

    /// Opens `path` for reading as another program would, sharing it with readers and writers
    /// and, if `share_delete`, with programs that delete or rename it.
    #[cfg(windows)]
    fn open_elsewhere(path: &Path, share_delete: bool) -> File {
        use std::os::windows::fs::OpenOptionsExt;

        const FILE_SHARE_READ: u32 = 1;
        const FILE_SHARE_WRITE: u32 = 2;
        const FILE_SHARE_DELETE: u32 = 4;
        let delete = if share_delete { FILE_SHARE_DELETE } else { 0 };
        fs::OpenOptions::new()
            .read(true)
            .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | delete)
            .open(path)
            .unwrap()
    }

    /// Obsidian, like most programs, opens files with every sharing mode.
    #[cfg(windows)]
    #[test]
    fn save_replaces_a_list_that_another_program_has_open_on_windows() {
        let (_directory, path) = workspace_with_list(ORIGINAL);
        let _open = open_elsewhere(&path, true);
        let document = TodoDocument::parse(EDITED);
        let expected = Fingerprint::of(ORIGINAL.as_bytes());

        let system_under_test = save(&path, &document, expected);

        assert!(system_under_test.is_ok());
        assert_eq!(fs::read_to_string(&path).unwrap(), EDITED);
    }

    #[cfg(windows)]
    #[test]
    fn save_reports_a_list_locked_against_renaming_on_windows() {
        let (directory, path) = workspace_with_list(ORIGINAL);
        let open = open_elsewhere(&path, false);
        let document = TodoDocument::parse(EDITED);
        let expected = Fingerprint::of(ORIGINAL.as_bytes());
        let expected_entries = vec![LIST_NAME.to_owned()];

        let system_under_test = save(&path, &document, expected);

        drop(open);
        assert!(matches!(
            system_under_test,
            Err(SaveError::Write { path: ref error_path, .. }) if *error_path == path
        ));
        assert_eq!(fs::read_to_string(&path).unwrap(), ORIGINAL);
        assert_eq!(entry_names(directory.path()), expected_entries);
    }

    #[cfg(unix)]
    #[rstest]
    #[case::owner_only(0o600)]
    #[case::world_readable(0o644)]
    #[case::group_readable(0o640)]
    fn save_keeps_the_file_permissions(#[case] mode: u32) {
        use std::os::unix::fs::PermissionsExt;

        let (_directory, path) = workspace_with_list(ORIGINAL);
        fs::set_permissions(&path, fs::Permissions::from_mode(mode)).unwrap();
        let document = TodoDocument::parse(EDITED);
        let expected = Fingerprint::of(ORIGINAL.as_bytes());
        save(&path, &document, expected).unwrap();

        let system_under_test = fs::metadata(&path).unwrap().permissions().mode() & 0o777;

        assert_eq!(system_under_test, mode);
    }

    #[cfg(unix)]
    #[test]
    fn save_through_a_symlink_keeps_the_link_and_updates_its_target() {
        let (directory, target) = workspace_with_list(ORIGINAL);
        let link = directory.path().join("link.md");
        std::os::unix::fs::symlink(LIST_NAME, &link).unwrap();
        let document = TodoDocument::parse(EDITED);
        let expected = Fingerprint::of(ORIGINAL.as_bytes());
        save(&link, &document, expected).unwrap();

        let system_under_test = fs::symlink_metadata(&link).unwrap();

        assert!(system_under_test.file_type().is_symlink());
        assert_eq!(fs::read_to_string(&target).unwrap(), EDITED);
        assert_eq!(
            entry_names(directory.path()),
            vec![LIST_NAME.to_owned(), "link.md".to_owned()]
        );
    }
}

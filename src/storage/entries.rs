//! Creating, renaming and deleting list files and folders of the workspace.
//!
//! Every new name is an [`EntryName`], which can only hold names that are safe as a single file
//! name and that Obsidian accepts. None of the operations ever replaces or recursively deletes
//! an existing entry. They only change the disk; updating the scanned
//! [`Workspace`](crate::storage::workspace::Workspace) is the caller's task.

use std::fmt::{self, Display, Formatter};
use std::fs::{self, OpenOptions};
use std::io::{self, ErrorKind};
use std::path::{Path, PathBuf};

use thiserror::Error;
use tracing::error;

/// File extension of new list files, appended to the [`EntryName`].
const LIST_EXTENSION: &str = ".md";

/// Longest file name most file systems accept, in bytes.
const MAX_FILE_NAME_BYTES: usize = 255;

/// Characters Obsidian forbids in file names, which include both path separators.
const FORBIDDEN_CHARACTERS: [char; 9] = ['*', '"', '\\', '/', '<', '>', ':', '|', '?'];

/// The validated name of a list or folder, without the `.md` extension of list files.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct EntryName(String);

impl EntryName {
    /// Returns the name without extension, as shown in the lists pane.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Returns the file name of a list with this name, including the `.md` extension.
    fn list_file_name(&self) -> String {
        [self.as_str(), LIST_EXTENSION].concat()
    }
}

impl TryFrom<&str> for EntryName {
    type Error = InvalidNameError;

    /// Parses a name typed by the user.
    ///
    /// Surrounding whitespace is trimmed and one trailing `.md` (in any case) is stripped, so
    /// typing `Groceries.md` names the list `Groceries`.
    ///
    /// # Errors
    ///
    /// Returns [`InvalidNameError::Empty`] if nothing is left,
    /// [`InvalidNameError::StartsWithDot`] for names starting with `.` (including `.` and `..`),
    /// which the workspace scan would hide, [`InvalidNameError::ForbiddenCharacter`] for path
    /// separators, control characters and the characters Obsidian forbids
    /// (`* " \ / < > : | ?`), and [`InvalidNameError::TooLong`] if the list file name would be
    /// longer than 255 bytes.
    fn try_from(input: &str) -> Result<Self, Self::Error> {
        let name = strip_list_extension(input.trim());
        if name.is_empty() {
            return Err(InvalidNameError::Empty);
        }
        if name.starts_with('.') {
            return Err(InvalidNameError::StartsWithDot);
        }
        if let Some(character) = name
            .chars()
            .find(|character| character.is_control() || FORBIDDEN_CHARACTERS.contains(character))
        {
            return Err(InvalidNameError::ForbiddenCharacter(character));
        }
        if name.len() + LIST_EXTENSION.len() > MAX_FILE_NAME_BYTES {
            return Err(InvalidNameError::TooLong);
        }
        Ok(Self(name.to_owned()))
    }
}

impl Display for EntryName {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

/// Removes one trailing `.md`, compared case-insensitively.
fn strip_list_extension(name: &str) -> &str {
    let Some(stem_length) = name.len().checked_sub(LIST_EXTENSION.len()) else {
        return name;
    };
    match name.split_at_checked(stem_length) {
        Some((stem, extension)) if extension.eq_ignore_ascii_case(LIST_EXTENSION) => stem,
        _ => name,
    }
}

/// Why a name typed by the user cannot name a list or folder.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
#[non_exhaustive]
pub enum InvalidNameError {
    /// The name is empty or only whitespace.
    #[error("the name is empty")]
    Empty,
    /// The name starts with `.`, which would hide the entry.
    #[error("names must not start with a dot")]
    StartsWithDot,
    /// The name contains a path separator, a control character or a character Obsidian
    /// forbids.
    #[error("names must not contain {0:?}")]
    ForbiddenCharacter(char),
    /// The list file name would be longer than 255 bytes.
    #[error("the name is longer than 255 bytes")]
    TooLong,
}

/// An error of an operation on a list file or folder.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum EntryError {
    /// The new name is not valid.
    #[error(transparent)]
    InvalidName(#[from] InvalidNameError),
    /// An entry with the new name already exists; it was left untouched.
    #[error("{} already exists", path.display())]
    AlreadyExists {
        /// The existing entry.
        path: PathBuf,
    },
    /// The folder to delete still contains entries, possibly hidden ones.
    #[error("folder {} is not empty", path.display())]
    NotEmpty {
        /// The folder that was not deleted.
        path: PathBuf,
    },
    /// The file system operation failed.
    #[error("cannot change {}", path.display())]
    Io {
        /// The entry the operation failed on.
        path: PathBuf,
        /// The underlying error.
        #[source]
        source: io::Error,
    },
}

/// Creates the empty list file `<name>.md` in `directory` and returns its path.
///
/// No heading is written because Obsidian shows the file name as the title.
///
/// # Errors
///
/// Returns [`EntryError::AlreadyExists`] if an entry with that file name exists, which is never
/// overwritten, and [`EntryError::Io`] if the file cannot be created.
///
/// # Examples
///
/// ```
/// use rudo::storage::entries::{EntryName, create_list};
///
/// let directory = tempfile::TempDir::new()?;
/// let name = EntryName::try_from("Groceries.md")?;
///
/// let path = create_list(directory.path(), &name)?;
///
/// assert_eq!(path, directory.path().join("Groceries.md"));
/// assert_eq!(std::fs::read_to_string(&path)?, "");
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
pub fn create_list(directory: &Path, name: &EntryName) -> Result<PathBuf, EntryError> {
    let path = directory.join(name.list_file_name());
    match OpenOptions::new().write(true).create_new(true).open(&path) {
        Ok(_) => Ok(path),
        Err(source) => Err(classify(path, source)),
    }
}

/// Creates the folder `name` in `directory` and returns its path.
///
/// # Errors
///
/// Returns [`EntryError::AlreadyExists`] if an entry with that name exists and
/// [`EntryError::Io`] if the folder cannot be created.
pub fn create_folder(directory: &Path, name: &EntryName) -> Result<PathBuf, EntryError> {
    let path = directory.join(name.as_str());
    match fs::create_dir(&path) {
        Ok(()) => Ok(path),
        Err(source) => Err(classify(path, source)),
    }
}

/// Renames the list file or folder at `path` to `name` in the same directory and returns the
/// new path.
///
/// A list file is renamed to `<name>.md`, a folder (a directory, not a symlink to one) to
/// `<name>`. Renaming only the case of a name (`groceries` to `Groceries`) works on
/// case-insensitive file systems too. `[[wikilinks]]` pointing to the entry are not updated.
///
/// Another program can still create an entry with the new name between the check and the
/// rename, which then replaces it; the standard library offers no rename that refuses to
/// replace, so the window is accepted.
///
/// # Errors
///
/// Returns [`EntryError::AlreadyExists`] if a different entry with the new name exists, which
/// is never overwritten, and [`EntryError::Io`] if `path` cannot be read or renamed.
pub fn rename(path: &Path, name: &EntryName) -> Result<PathBuf, EntryError> {
    let metadata = fs::symlink_metadata(path).map_err(|source| classify(path.into(), source))?;
    let file_name = if metadata.is_dir() {
        name.as_str().to_owned()
    } else {
        name.list_file_name()
    };
    let target = path.with_file_name(file_name);
    if target == path {
        return Ok(target);
    }
    match fs::symlink_metadata(&target) {
        Err(error) if error.kind() == ErrorKind::NotFound => fs::rename(path, &target),
        Err(source) => return Err(classify(target, source)),
        Ok(existing) if is_same_entry(path, &metadata, &target, &existing) => {
            rename_through_staging(path, &target)
        }
        Ok(_) => return Err(EntryError::AlreadyExists { path: target }),
    }
    .map_err(|source| classify(path.into(), source))?;
    Ok(target)
}

/// Deletes the list file at `path`.
///
/// # Errors
///
/// Returns [`EntryError::Io`] if the file cannot be deleted, including when `path` is a folder.
pub fn delete_list(path: &Path) -> Result<(), EntryError> {
    fs::remove_file(path).map_err(|source| classify(path.into(), source))
}

/// Deletes the folder at `path` if it is empty; folders are never deleted recursively.
///
/// # Errors
///
/// Returns [`EntryError::NotEmpty`] if the folder contains any entry, including hidden ones
/// that the lists pane does not show, and [`EntryError::Io`] if it cannot be deleted.
pub fn delete_folder(path: &Path) -> Result<(), EntryError> {
    fs::remove_dir(path).map_err(|source| classify(path.into(), source))
}

/// Turns an I/O error on `path` into the matching [`EntryError`].
fn classify(path: PathBuf, source: io::Error) -> EntryError {
    match source.kind() {
        ErrorKind::AlreadyExists => EntryError::AlreadyExists { path },
        ErrorKind::DirectoryNotEmpty => EntryError::NotEmpty { path },
        _ => EntryError::Io { path, source },
    }
}

/// Whether `source` and the existing `target` are the same directory entry, which happens for
/// a case-only rename on a case-insensitive file system.
#[cfg(unix)]
fn is_same_entry(
    _source: &Path,
    source_metadata: &fs::Metadata,
    _target: &Path,
    target_metadata: &fs::Metadata,
) -> bool {
    use std::os::unix::fs::MetadataExt;

    source_metadata.dev() == target_metadata.dev() && source_metadata.ino() == target_metadata.ino()
}

/// Whether `source` and the existing `target` are the same directory entry, which happens for
/// a case-only rename on a case-insensitive file system.
#[cfg(not(unix))]
fn is_same_entry(
    source: &Path,
    _source_metadata: &fs::Metadata,
    target: &Path,
    _target_metadata: &fs::Metadata,
) -> bool {
    match (fs::canonicalize(source), fs::canonicalize(target)) {
        (Ok(source), Ok(target)) => source == target,
        _ => false,
    }
}

/// Renames `source` to `target` in the same directory through a hidden staging folder next to them, for file systems
/// that treat both names as the same entry.
///
/// If the second step fails the entry is moved back. The staging folder is only removed while
/// empty, so the entry is never deleted with it.
fn rename_through_staging(source: &Path, target: &Path) -> io::Result<()> {
    let parent = target
        .parent()
        .expect("a path built with a file name has a parent directory");
    let staging = tempfile::Builder::new()
        .prefix(".rudo-rename")
        .tempdir_in(parent)?
        .keep();
    let staged = staging.join("entry");
    fs::rename(source, &staged)?;
    if let Err(rename_error) = fs::rename(&staged, target) {
        if let Err(restore_error) = fs::rename(&staged, source) {
            error!(
                staged = %staged.display(),
                %restore_error,
                "cannot move the entry back after a failed rename"
            );
            return Err(rename_error);
        }
        fs::remove_dir(&staging)?;
        return Err(rename_error);
    }
    fs::remove_dir(&staging)
}

#[cfg(test)]
mod tests {
    use rstest::rstest;
    use tempfile::TempDir;

    use super::*;

    fn name(text: &str) -> EntryName {
        EntryName::try_from(text).unwrap()
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

    #[rstest]
    #[case::plain("Groceries", "Groceries")]
    #[case::trimmed("  Groceries \t", "Groceries")]
    #[case::extension_stripped("Groceries.md", "Groceries")]
    #[case::extension_stripped_case_insensitively("Groceries.MD", "Groceries")]
    #[case::extension_trimmed(" Groceries.Md ", "Groceries")]
    #[case::only_one_extension_stripped("notes.md.md", "notes.md")]
    #[case::other_extension_kept("archive.txt", "archive.txt")]
    #[case::inner_dot("v1.0 release", "v1.0 release")]
    #[case::trailing_dot("Done.", "Done.")]
    #[case::multi_byte("Einkäufe 🛒", "Einkäufe 🛒")]
    #[case::multi_byte_before_extension("äö.md", "äö")]
    #[case::short("md", "md")]
    #[case::longest(&"a".repeat(252), &"a".repeat(252))]
    #[case::longest_with_extension(&format!("{}.md", "a".repeat(252)), &"a".repeat(252))]
    fn entry_name_accepts_valid_names(#[case] input: &str, #[case] expected: &str) {
        let system_under_test = EntryName::try_from(input).unwrap();

        assert_eq!(system_under_test.as_str(), expected);
    }

    #[rstest]
    #[case::empty("", InvalidNameError::Empty)]
    #[case::whitespace(" \t ", InvalidNameError::Empty)]
    #[case::only_extension(".md", InvalidNameError::Empty)]
    #[case::current_directory(".", InvalidNameError::StartsWithDot)]
    #[case::parent_directory("..", InvalidNameError::StartsWithDot)]
    #[case::hidden(".obsidian", InvalidNameError::StartsWithDot)]
    #[case::hidden_list(".secret.md", InvalidNameError::StartsWithDot)]
    #[case::slash("a/b", InvalidNameError::ForbiddenCharacter('/'))]
    #[case::backslash("a\\b", InvalidNameError::ForbiddenCharacter('\\'))]
    #[case::asterisk("a*", InvalidNameError::ForbiddenCharacter('*'))]
    #[case::quote("a\"b", InvalidNameError::ForbiddenCharacter('"'))]
    #[case::less_than("a<b", InvalidNameError::ForbiddenCharacter('<'))]
    #[case::greater_than("a>b", InvalidNameError::ForbiddenCharacter('>'))]
    #[case::colon("a:b", InvalidNameError::ForbiddenCharacter(':'))]
    #[case::pipe("a|b", InvalidNameError::ForbiddenCharacter('|'))]
    #[case::question_mark("a?", InvalidNameError::ForbiddenCharacter('?'))]
    #[case::tab("a\tb", InvalidNameError::ForbiddenCharacter('\t'))]
    #[case::newline("a\nb", InvalidNameError::ForbiddenCharacter('\n'))]
    #[case::nul("a\0b", InvalidNameError::ForbiddenCharacter('\0'))]
    #[case::delete("a\u{7f}", InvalidNameError::ForbiddenCharacter('\u{7f}'))]
    #[case::too_long(&"a".repeat(253), InvalidNameError::TooLong)]
    #[case::too_long_multi_byte(&"ä".repeat(127), InvalidNameError::TooLong)]
    fn entry_name_rejects_invalid_names(#[case] input: &str, #[case] expected: InvalidNameError) {
        let system_under_test = EntryName::try_from(input);

        assert_eq!(system_under_test, Err(expected));
    }

    #[test]
    fn entry_name_displays_without_extension() {
        let input = "Groceries.md";
        let expected = "Groceries";

        let system_under_test = name(input);

        assert_eq!(system_under_test.to_string(), expected);
    }

    #[test]
    fn invalid_name_converts_into_entry_error() {
        let invalid = InvalidNameError::Empty;

        let system_under_test = EntryError::from(invalid);

        assert!(matches!(
            system_under_test,
            EntryError::InvalidName(InvalidNameError::Empty)
        ));
    }

    #[test]
    fn create_list_creates_an_empty_markdown_file() {
        let directory = TempDir::new().unwrap();
        let list_name = name("Groceries");
        let expected = directory.path().join("Groceries.md");

        let system_under_test = create_list(directory.path(), &list_name).unwrap();

        assert_eq!(system_under_test, expected);
        assert_eq!(fs::read(&expected).unwrap(), b"");
    }

    #[test]
    fn create_list_does_not_overwrite_an_existing_list() {
        let directory = TempDir::new().unwrap();
        let existing = directory.path().join("Groceries.md");
        fs::write(&existing, "- [ ] Buy milk\n").unwrap();
        let list_name = name("Groceries");

        let system_under_test = create_list(directory.path(), &list_name);

        assert!(matches!(
            system_under_test,
            Err(EntryError::AlreadyExists { ref path }) if *path == existing
        ));
        assert_eq!(fs::read_to_string(&existing).unwrap(), "- [ ] Buy milk\n");
    }

    #[test]
    fn create_list_reports_a_missing_directory() {
        let directory = TempDir::new().unwrap();
        let missing = directory.path().join("missing");
        let list_name = name("Groceries");

        let system_under_test = create_list(&missing, &list_name);

        assert!(matches!(
            system_under_test,
            Err(EntryError::Io { ref source, .. }) if source.kind() == ErrorKind::NotFound
        ));
    }

    #[test]
    fn create_folder_creates_a_directory() {
        let directory = TempDir::new().unwrap();
        let folder_name = name("Projects");
        let expected = directory.path().join("Projects");

        let system_under_test = create_folder(directory.path(), &folder_name).unwrap();

        assert_eq!(system_under_test, expected);
        assert!(expected.is_dir());
    }

    #[test]
    fn create_folder_does_not_replace_an_existing_entry() {
        let directory = TempDir::new().unwrap();
        let existing = directory.path().join("Projects");
        fs::write(&existing, "").unwrap();
        let folder_name = name("Projects");

        let system_under_test = create_folder(directory.path(), &folder_name);

        assert!(matches!(
            system_under_test,
            Err(EntryError::AlreadyExists { ref path }) if *path == existing
        ));
        assert!(existing.is_file());
    }

    #[test]
    fn create_folder_reports_a_missing_directory() {
        let directory = TempDir::new().unwrap();
        let missing = directory.path().join("missing");
        let folder_name = name("Projects");

        let system_under_test = create_folder(&missing, &folder_name);

        assert!(matches!(system_under_test, Err(EntryError::Io { .. })));
    }

    #[test]
    fn rename_renames_a_list_and_keeps_its_content() {
        let directory = TempDir::new().unwrap();
        let source = directory.path().join("groceries.md");
        fs::write(&source, "- [ ] Buy milk\n").unwrap();
        let new_name = name("Shopping");
        let expected = directory.path().join("Shopping.md");

        let system_under_test = rename(&source, &new_name).unwrap();

        assert_eq!(system_under_test, expected);
        assert_eq!(fs::read_to_string(&expected).unwrap(), "- [ ] Buy milk\n");
        assert_eq!(entry_names(directory.path()), vec!["Shopping.md"]);
    }

    #[test]
    fn rename_renames_a_folder_without_extension() {
        let directory = TempDir::new().unwrap();
        let source = directory.path().join("projects");
        fs::create_dir(&source).unwrap();
        fs::write(source.join("home.md"), "").unwrap();
        let new_name = name("Work");
        let expected = directory.path().join("Work");

        let system_under_test = rename(&source, &new_name).unwrap();

        assert_eq!(system_under_test, expected);
        assert_eq!(entry_names(&expected), vec!["home.md"]);
        assert_eq!(entry_names(directory.path()), vec!["Work"]);
    }

    #[rstest]
    #[case::list("groceries.md", "Groceries", "Groceries.md")]
    #[case::folder("projects", "Projects", "Projects")]
    fn rename_changes_only_the_case(
        #[case] original: &str,
        #[case] new_name: &str,
        #[case] expected: &str,
    ) {
        let directory = TempDir::new().unwrap();
        let source = directory.path().join(original);
        if Path::new(original).extension().is_some() {
            fs::write(&source, "content").unwrap();
        } else {
            fs::create_dir(&source).unwrap();
        }
        let new_name = name(new_name);
        let expected_path = directory.path().join(expected);

        let system_under_test = rename(&source, &new_name).unwrap();

        assert_eq!(system_under_test, expected_path);
        assert_eq!(entry_names(directory.path()), vec![expected]);
    }

    #[test]
    fn rename_to_the_same_name_changes_nothing() {
        let directory = TempDir::new().unwrap();
        let source = directory.path().join("Groceries.md");
        fs::write(&source, "content").unwrap();
        let new_name = name("Groceries");

        let system_under_test = rename(&source, &new_name).unwrap();

        assert_eq!(system_under_test, source);
        assert_eq!(fs::read_to_string(&source).unwrap(), "content");
    }

    #[rstest]
    #[case::list_onto_list("a.md", "b.md", "b")]
    #[case::folder_onto_file("a", "b", "b")]
    fn rename_does_not_overwrite_an_existing_entry(
        #[case] original: &str,
        #[case] existing: &str,
        #[case] new_name: &str,
    ) {
        let directory = TempDir::new().unwrap();
        let source = directory.path().join(original);
        let existing_path = directory.path().join(existing);
        if Path::new(original).extension().is_some() {
            fs::write(&source, "source").unwrap();
        } else {
            fs::create_dir(&source).unwrap();
        }
        fs::write(&existing_path, "existing").unwrap();
        let new_name = name(new_name);

        let system_under_test = rename(&source, &new_name);

        assert!(matches!(
            system_under_test,
            Err(EntryError::AlreadyExists { ref path }) if *path == existing_path
        ));
        assert!(source.exists());
        assert_eq!(fs::read_to_string(&existing_path).unwrap(), "existing");
    }

    #[test]
    fn rename_reports_a_missing_entry() {
        let directory = TempDir::new().unwrap();
        let missing = directory.path().join("missing.md");
        let new_name = name("Other");

        let system_under_test = rename(&missing, &new_name);

        assert!(matches!(
            system_under_test,
            Err(EntryError::Io { ref path, ref source })
                if *path == missing && source.kind() == ErrorKind::NotFound
        ));
    }

    #[rstest]
    #[case::list(false)]
    #[case::folder(true)]
    fn rename_through_staging_moves_the_entry_and_leaves_no_staging_folder(
        #[case] is_folder: bool,
    ) {
        let directory = TempDir::new().unwrap();
        let source = directory.path().join("groceries");
        if is_folder {
            fs::create_dir(&source).unwrap();
        } else {
            fs::write(&source, "content").unwrap();
        }
        let target = directory.path().join("Groceries");

        let system_under_test = rename_through_staging(&source, &target);

        assert!(system_under_test.is_ok());
        assert_eq!(entry_names(directory.path()), vec!["Groceries"]);
    }

    #[test]
    fn rename_through_staging_moves_the_entry_back_when_the_target_is_unusable() {
        let directory = TempDir::new().unwrap();
        let source = directory.path().join("groceries");
        fs::write(&source, "content").unwrap();
        let target = directory.path().join("missing").join("Groceries");

        let system_under_test = rename_through_staging(&source, &target);

        assert!(system_under_test.is_err());
        assert_eq!(entry_names(directory.path()), vec!["groceries"]);
        assert_eq!(fs::read_to_string(&source).unwrap(), "content");
    }

    #[test]
    fn delete_list_removes_the_file() {
        let directory = TempDir::new().unwrap();
        let list = directory.path().join("Groceries.md");
        fs::write(&list, "- [ ] Buy milk\n").unwrap();

        let system_under_test = delete_list(&list);

        assert!(system_under_test.is_ok());
        assert!(!list.exists());
    }

    #[test]
    fn delete_list_reports_a_missing_file() {
        let directory = TempDir::new().unwrap();
        let missing = directory.path().join("missing.md");

        let system_under_test = delete_list(&missing);

        assert!(matches!(
            system_under_test,
            Err(EntryError::Io { ref path, .. }) if *path == missing
        ));
    }

    #[test]
    fn delete_list_refuses_a_folder() {
        let directory = TempDir::new().unwrap();
        let folder = directory.path().join("Projects");
        fs::create_dir(&folder).unwrap();

        let system_under_test = delete_list(&folder);

        assert!(matches!(system_under_test, Err(EntryError::Io { .. })));
        assert!(folder.is_dir());
    }

    #[test]
    fn delete_folder_removes_an_empty_folder() {
        let directory = TempDir::new().unwrap();
        let folder = directory.path().join("Projects");
        fs::create_dir(&folder).unwrap();

        let system_under_test = delete_folder(&folder);

        assert!(system_under_test.is_ok());
        assert!(!folder.exists());
    }

    #[rstest]
    #[case::list("home.md")]
    #[case::hidden_entry(".obsidian")]
    fn delete_folder_keeps_a_folder_that_is_not_empty(#[case] content: &str) {
        let directory = TempDir::new().unwrap();
        let folder = directory.path().join("Projects");
        fs::create_dir(&folder).unwrap();
        fs::write(folder.join(content), "").unwrap();

        let system_under_test = delete_folder(&folder);

        assert!(matches!(
            system_under_test,
            Err(EntryError::NotEmpty { ref path }) if *path == folder
        ));
        assert_eq!(entry_names(&folder), vec![content]);
    }

    #[test]
    fn delete_folder_reports_a_missing_folder() {
        let directory = TempDir::new().unwrap();
        let missing = directory.path().join("missing");

        let system_under_test = delete_folder(&missing);

        assert!(matches!(system_under_test, Err(EntryError::Io { .. })));
    }

    #[rstest]
    #[case::already_exists(ErrorKind::AlreadyExists, "a already exists")]
    #[case::not_empty(ErrorKind::DirectoryNotEmpty, "folder a is not empty")]
    #[case::other(ErrorKind::PermissionDenied, "cannot change a")]
    fn entry_errors_have_concise_messages(#[case] kind: ErrorKind, #[case] expected: &str) {
        let source = io::Error::from(kind);

        let system_under_test = classify(PathBuf::from("a"), source);

        assert_eq!(system_under_test.to_string(), expected);
    }
}

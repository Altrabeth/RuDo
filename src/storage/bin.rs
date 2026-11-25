//! The bin: a folder inside the workspace that receives deleted lists and folders, so that a
//! deletion can be undone.
//!
//! The bin is a plain folder, `.trash/` by default, the folder Obsidian itself uses for deleted
//! notes. An entry keeps its path relative to the workspace root inside the bin
//! (`Home/Groceries.md` becomes `.trash/Home/Groceries.md`), and moving it there is a rename on
//! the same file system, so it is atomic and keeps the identity of the file. The scan and the
//! live reload ignore the bin, wherever it is configured.

use std::ffi::{OsStr, OsString};
use std::fs;
use std::io::{self, ErrorKind};
use std::path::{Component, Path, PathBuf};

use serde::Deserialize;
use thiserror::Error;

use crate::storage::entries::{EntryError, classify, move_to};

/// The bin of a workspace that is not configured otherwise.
const DEFAULT_BIN: &str = ".trash";

/// The path of the bin relative to the workspace root, `.trash` by default.
///
/// It only holds a relative path inside the root that is not the root itself, so RuDo never
/// scans its own bin or moves the workspace into it.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Deserialize)]
#[serde(try_from = "PathBuf")]
pub struct BinPath(PathBuf);

impl BinPath {
    /// Returns the path relative to the workspace root.
    #[must_use]
    pub fn as_path(&self) -> &Path {
        &self.0
    }

    /// Returns whether `path`, relative to the workspace root, is the bin or inside it.
    #[must_use]
    pub fn contains(&self, path: &Path) -> bool {
        path.starts_with(&self.0)
    }
}

impl Default for BinPath {
    fn default() -> Self {
        Self(PathBuf::from(DEFAULT_BIN))
    }
}

impl TryFrom<PathBuf> for BinPath {
    type Error = InvalidBinError;

    /// Parses the configured `workspace.bin`; `.` components are dropped.
    ///
    /// # Errors
    ///
    /// Returns [`InvalidBinError::Absolute`] for an absolute path,
    /// [`InvalidBinError::OutsideRoot`] for a path with `..` and [`InvalidBinError::Root`] for
    /// a path that names the workspace root itself.
    fn try_from(path: PathBuf) -> Result<Self, Self::Error> {
        let mut relative = PathBuf::with_capacity(path.as_os_str().len());
        for component in path.components() {
            match component {
                Component::Prefix(_) | Component::RootDir => return Err(InvalidBinError::Absolute),
                Component::ParentDir => return Err(InvalidBinError::OutsideRoot),
                Component::CurDir => {}
                Component::Normal(name) => relative.push(name),
            }
        }
        if relative.as_os_str().is_empty() {
            return Err(InvalidBinError::Root);
        }
        Ok(Self(relative))
    }
}

/// Why a configured `workspace.bin` cannot be the bin.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Error)]
#[non_exhaustive]
pub enum InvalidBinError {
    /// The path is absolute.
    #[error("the bin must be a path relative to the workspace folder")]
    Absolute,
    /// The path leaves the workspace folder with `..`.
    #[error("the bin must be inside the workspace folder")]
    OutsideRoot,
    /// The path is empty or names the workspace folder itself.
    #[error("the bin must not be the workspace folder itself")]
    Root,
}

/// A list file or folder that RuDo moved into the bin: where it came from and where it is.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct TrashedEntry {
    original: PathBuf,
    location: PathBuf,
}

impl TrashedEntry {
    /// Returns the path the entry had before, relative to the workspace root.
    #[must_use]
    pub fn original(&self) -> &Path {
        &self.original
    }

    /// Returns the path of the entry relative to the bin.
    #[must_use]
    pub fn location(&self) -> &Path {
        &self.location
    }
}

/// Moves the list file or folder at `path`, relative to the workspace `root`, into the bin and
/// returns where it went.
///
/// The entry keeps its path relative to the root inside the bin; the folders it needs there
/// are created. If an entry with that name is already in the bin, the moved one is numbered
/// (`Groceries 2.md`, `Work 2`), so nothing in the bin is ever replaced. A symlink is moved
/// itself, not the file it points to.
///
/// # Errors
///
/// Returns [`EntryError::Io`] if the entry cannot be read or moved or the folder in the bin
/// cannot be created, and [`EntryError::IntoItself`] if `path` is a folder that contains the
/// bin.
///
/// # Examples
///
/// ```
/// use std::path::Path;
///
/// use rudo::storage::bin::{BinPath, move_to_bin, restore};
///
/// let root = tempfile::TempDir::new()?;
/// std::fs::create_dir(root.path().join("Home"))?;
/// std::fs::write(root.path().join("Home/Groceries.md"), "- [ ] Milk\n")?;
/// let bin = BinPath::default();
///
/// let trashed = move_to_bin(root.path(), &bin, Path::new("Home/Groceries.md"))?;
/// assert_eq!(trashed.location(), Path::new("Home/Groceries.md"));
/// assert!(root.path().join(".trash/Home/Groceries.md").is_file());
///
/// restore(root.path(), &bin, &trashed)?;
/// assert_eq!(std::fs::read_to_string(root.path().join("Home/Groceries.md"))?, "- [ ] Milk\n");
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
pub fn move_to_bin(root: &Path, bin: &BinPath, path: &Path) -> Result<TrashedEntry, EntryError> {
    let source = root.join(path);
    let metadata =
        fs::symlink_metadata(&source).map_err(|error| classify(source.clone(), error))?;
    let (Some(folder), Some(file_name)) = (path.parent(), path.file_name()) else {
        return Err(classify(source, io::Error::from(ErrorKind::InvalidInput)));
    };
    let bin_folder = root.join(&bin.0);
    let directory = bin_folder.join(folder);
    fs::create_dir_all(&directory).map_err(|error| classify(directory, error))?;
    let mut number = 1;
    loop {
        let location = folder.join(numbered_name(file_name, metadata.is_dir(), number));
        match move_to(&source, &bin_folder.join(&location)) {
            Ok(_) => {
                return Ok(TrashedEntry {
                    original: path.to_path_buf(),
                    location,
                });
            }
            Err(EntryError::AlreadyExists { .. }) => number += 1,
            Err(error) => return Err(error),
        }
    }
}

/// Returns `file_name` for the first entry of that name in the bin and, from `number` 2 on,
/// with the number before the extension of a file (`Groceries 2.md`) or after the name of a
/// folder (`Work 2`).
fn numbered_name(file_name: &OsStr, is_folder: bool, number: u32) -> OsString {
    if number == 1 {
        return file_name.to_os_string();
    }
    let path = Path::new(file_name);
    let (stem, extension) = match (is_folder, path.file_stem(), path.extension()) {
        (false, Some(stem), Some(extension)) => (stem, Some(extension)),
        _ => (file_name, None),
    };
    let mut name = stem.to_os_string();
    name.push(format!(" {number}"));
    if let Some(extension) = extension {
        name.push(".");
        name.push(extension);
    }
    name
}

/// Moves `entry` from the bin of the workspace `root` back to its original path and returns
/// that path.
///
/// Folders on the way to the original path that no longer exist are created, and folders in
/// the bin that the entry leaves empty are removed.
///
/// # Errors
///
/// Returns [`EntryError::AlreadyExists`] if another entry took the original path, which is
/// never replaced, and [`EntryError::Io`] if the entry is no longer in the bin or cannot be
/// moved. The entry then stays in the bin.
pub fn restore(root: &Path, bin: &BinPath, entry: &TrashedEntry) -> Result<PathBuf, EntryError> {
    let bin_folder = root.join(&bin.0);
    let source = bin_folder.join(&entry.location);
    fs::symlink_metadata(&source).map_err(|error| classify(source.clone(), error))?;
    let target = root.join(&entry.original);
    if let Some(folder) = target.parent() {
        fs::create_dir_all(folder).map_err(|error| classify(folder.to_path_buf(), error))?;
    }
    move_to(&source, &target)?;
    remove_empty_folders(&bin_folder, &entry.location);
    Ok(target)
}

/// Removes the folders of the bin `bin_folder` that contain `location` while they are empty,
/// innermost first; the bin itself stays.
fn remove_empty_folders(bin_folder: &Path, location: &Path) {
    for folder in location
        .ancestors()
        .skip(1)
        .take_while(|folder| !folder.as_os_str().is_empty())
    {
        let absolute = bin_folder.join(folder);
        if let Err(error) = fs::remove_dir(&absolute) {
            // A folder that still holds other deleted entries is kept on purpose.
            tracing::debug!(folder = %absolute.display(), %error, "folder in the bin kept");
            return;
        }
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;
    use tempfile::TempDir;

    use super::*;

    fn bin() -> BinPath {
        BinPath::default()
    }

    /// Creates the files `(path, content)` and the folders (paths ending in `/`) in `root`.
    fn create(root: &Path, entries: &[(&str, &str)]) {
        for (path, content) in entries {
            let absolute = root.join(path);
            if path.ends_with('/') {
                fs::create_dir_all(absolute).unwrap();
            } else {
                fs::create_dir_all(absolute.parent().unwrap()).unwrap();
                fs::write(absolute, content).unwrap();
            }
        }
    }

    #[rstest]
    #[case::hidden(".trash", ".trash")]
    #[case::nested("Archive/Bin", "Archive/Bin")]
    #[case::current_folder_dropped("./.trash/.", ".trash")]
    fn bin_path_accepts_relative_paths_inside_the_root(
        #[case] input: &str,
        #[case] expected: &str,
    ) {
        let system_under_test = BinPath::try_from(PathBuf::from(input)).unwrap();

        assert_eq!(system_under_test.as_path(), Path::new(expected));
    }

    #[rstest]
    #[case::absolute("/srv/trash", InvalidBinError::Absolute)]
    #[case::parent("../trash", InvalidBinError::OutsideRoot)]
    #[case::inner_parent("a/../../trash", InvalidBinError::OutsideRoot)]
    #[case::empty("", InvalidBinError::Root)]
    #[case::root(".", InvalidBinError::Root)]
    fn bin_path_rejects_paths_outside_or_at_the_root(
        #[case] input: &str,
        #[case] expected: InvalidBinError,
    ) {
        let system_under_test = BinPath::try_from(PathBuf::from(input));

        assert_eq!(system_under_test, Err(expected));
    }

    #[test]
    fn default_bin_is_the_trash_folder_of_obsidian() {
        let expected = Path::new(".trash");

        let system_under_test = BinPath::default();

        assert_eq!(system_under_test.as_path(), expected);
    }

    #[rstest]
    #[case::bin(".trash", true)]
    #[case::inside(".trash/Home/Groceries.md", true)]
    #[case::similar_name(".trash2", false)]
    #[case::trash_in_a_folder("Home/.trash", false)]
    #[case::root("", false)]
    #[case::other("Home/Groceries.md", false)]
    fn bin_contains_itself_and_everything_inside(#[case] path: &str, #[case] expected: bool) {
        let system_under_test = bin().contains(Path::new(path));

        assert_eq!(system_under_test, expected);
    }

    #[rstest]
    #[case::list("Groceries.md", "Groceries.md")]
    #[case::nested_list("Home/Kitchen/Groceries.md", "Home/Kitchen/Groceries.md")]
    #[case::folder("Work", "Work")]
    fn move_to_bin_keeps_the_path_inside_the_bin(#[case] path: &str, #[case] expected: &str) {
        let root = TempDir::new().unwrap();
        create(
            root.path(),
            &[
                ("Groceries.md", "- [ ] Milk\n"),
                ("Home/Kitchen/Groceries.md", "- [ ] Eggs\n"),
                ("Work/Plan.md", "- [ ] Write\n"),
            ],
        );
        let expected_location = Path::new(expected);

        let system_under_test = move_to_bin(root.path(), &bin(), Path::new(path)).unwrap();

        assert_eq!(system_under_test.original(), Path::new(path));
        assert_eq!(system_under_test.location(), expected_location);
        assert!(!root.path().join(path).exists());
        assert!(root.path().join(".trash").join(expected).exists());
    }

    #[test]
    fn move_to_bin_moves_a_folder_with_everything_inside() {
        let root = TempDir::new().unwrap();
        create(
            root.path(),
            &[("Work/Plan.md", "- [ ] Write\n"), ("Work/Old/", "")],
        );
        let expected = "- [ ] Write\n";

        let system_under_test = move_to_bin(root.path(), &bin(), Path::new("Work")).unwrap();

        assert_eq!(system_under_test.location(), Path::new("Work"));
        assert_eq!(
            fs::read_to_string(root.path().join(".trash/Work/Plan.md")).unwrap(),
            expected
        );
        assert!(root.path().join(".trash/Work/Old").is_dir());
    }

    #[rstest]
    #[case::list(&[".trash/Groceries.md"], "Groceries.md", "Groceries 2.md")]
    #[case::third_list(
        &[".trash/Groceries.md", ".trash/Groceries 2.md"],
        "Groceries.md",
        "Groceries 3.md"
    )]
    #[case::folder(&[".trash/Work/"], "Work/", "Work 2")]
    #[case::folder_with_dot(&[".trash/v1.0/"], "v1.0/", "v1.0 2")]
    fn move_to_bin_numbers_a_name_already_in_the_bin(
        #[case] in_bin: &[&str],
        #[case] path: &str,
        #[case] expected: &str,
    ) {
        let root = TempDir::new().unwrap();
        let existing: Vec<(&str, &str)> = in_bin.iter().map(|path| (*path, "old")).collect();
        create(root.path(), &existing);
        create(root.path(), &[(path, "new")]);
        let path = path.trim_end_matches('/');

        let system_under_test = move_to_bin(root.path(), &bin(), Path::new(path)).unwrap();

        assert_eq!(system_under_test.location(), Path::new(expected));
        for old in in_bin {
            assert!(root.path().join(old).exists(), "{old} was replaced");
        }
    }

    #[test]
    fn move_to_bin_creates_a_configured_bin_on_demand() {
        let root = TempDir::new().unwrap();
        create(root.path(), &[("Groceries.md", "")]);
        let bin = BinPath::try_from(PathBuf::from("Archive/Bin")).unwrap();

        let system_under_test = move_to_bin(root.path(), &bin, Path::new("Groceries.md"));

        assert!(system_under_test.is_ok());
        assert!(root.path().join("Archive/Bin/Groceries.md").is_file());
    }

    #[test]
    fn move_to_bin_refuses_a_folder_that_contains_the_bin() {
        let root = TempDir::new().unwrap();
        create(root.path(), &[("Archive/Plan.md", "")]);
        let bin = BinPath::try_from(PathBuf::from("Archive/Bin")).unwrap();

        let system_under_test = move_to_bin(root.path(), &bin, Path::new("Archive"));

        assert!(matches!(
            system_under_test,
            Err(EntryError::IntoItself { .. })
        ));
        assert!(root.path().join("Archive/Plan.md").is_file());
    }

    #[test]
    fn move_to_bin_reports_a_missing_entry() {
        let root = TempDir::new().unwrap();

        let system_under_test = move_to_bin(root.path(), &bin(), Path::new("Gone.md"));

        assert!(matches!(
            system_under_test,
            Err(EntryError::Io { ref source, .. }) if source.kind() == ErrorKind::NotFound
        ));
        assert!(!root.path().join(".trash").exists());
    }

    #[test]
    fn restore_moves_the_entry_back_and_removes_emptied_bin_folders() {
        let root = TempDir::new().unwrap();
        create(
            root.path(),
            &[
                ("Home/Kitchen/Groceries.md", "- [ ] Milk\n"),
                (".trash/Home/Other.md", ""),
            ],
        );
        let trashed =
            move_to_bin(root.path(), &bin(), Path::new("Home/Kitchen/Groceries.md")).unwrap();
        let expected = root.path().join("Home/Kitchen/Groceries.md");

        let system_under_test = restore(root.path(), &bin(), &trashed).unwrap();

        assert_eq!(system_under_test, expected);
        assert_eq!(fs::read_to_string(&expected).unwrap(), "- [ ] Milk\n");
        assert!(!root.path().join(".trash/Home/Kitchen").exists());
        assert!(root.path().join(".trash/Home/Other.md").is_file());
    }

    #[test]
    fn restore_recreates_missing_folders_of_the_original_path() {
        let root = TempDir::new().unwrap();
        create(root.path(), &[("Home/Groceries.md", "- [ ] Milk\n")]);
        let trashed = move_to_bin(root.path(), &bin(), Path::new("Home/Groceries.md")).unwrap();
        fs::remove_dir(root.path().join("Home")).unwrap();

        let system_under_test = restore(root.path(), &bin(), &trashed);

        assert!(system_under_test.is_ok());
        assert!(root.path().join("Home/Groceries.md").is_file());
    }

    #[test]
    fn restore_never_replaces_an_entry_at_the_original_path() {
        let root = TempDir::new().unwrap();
        create(root.path(), &[("Groceries.md", "old")]);
        let trashed = move_to_bin(root.path(), &bin(), Path::new("Groceries.md")).unwrap();
        create(root.path(), &[("Groceries.md", "new")]);

        let system_under_test = restore(root.path(), &bin(), &trashed);

        assert!(matches!(
            system_under_test,
            Err(EntryError::AlreadyExists { .. })
        ));
        assert_eq!(
            fs::read_to_string(root.path().join("Groceries.md")).unwrap(),
            "new"
        );
        assert_eq!(
            fs::read_to_string(root.path().join(".trash/Groceries.md")).unwrap(),
            "old"
        );
    }

    #[test]
    fn restore_reports_an_entry_gone_from_the_bin() {
        let root = TempDir::new().unwrap();
        create(root.path(), &[("Groceries.md", "")]);
        let trashed = move_to_bin(root.path(), &bin(), Path::new("Groceries.md")).unwrap();
        fs::remove_file(root.path().join(".trash/Groceries.md")).unwrap();

        let system_under_test = restore(root.path(), &bin(), &trashed);

        assert!(matches!(system_under_test, Err(EntryError::Io { .. })));
        assert!(!root.path().join("Groceries.md").exists());
    }

    #[rstest]
    #[case::list("Groceries.md", false, 2, "Groceries 2.md")]
    #[case::list_with_dots("notes.md.md", false, 3, "notes.md 3.md")]
    #[case::file_without_extension("README", false, 2, "README 2")]
    #[case::folder("v1.0", true, 2, "v1.0 2")]
    #[case::first("Groceries.md", false, 1, "Groceries.md")]
    fn numbered_names_keep_the_extension_of_files(
        #[case] file_name: &str,
        #[case] is_folder: bool,
        #[case] number: u32,
        #[case] expected: &str,
    ) {
        let system_under_test = numbered_name(OsStr::new(file_name), is_folder, number);

        assert_eq!(system_under_test, OsStr::new(expected));
    }

    #[cfg(unix)]
    #[test]
    fn symlinked_list_moves_as_a_link_and_comes_back() {
        let root = TempDir::new().unwrap();
        let outside = TempDir::new().unwrap();
        let target = outside.path().join("Real.md");
        fs::write(&target, "- [ ] Milk\n").unwrap();
        std::os::unix::fs::symlink(&target, root.path().join("Link.md")).unwrap();
        let trashed = move_to_bin(root.path(), &bin(), Path::new("Link.md")).unwrap();
        let in_bin = fs::symlink_metadata(root.path().join(".trash/Link.md")).unwrap();

        let system_under_test = restore(root.path(), &bin(), &trashed);

        assert!(in_bin.file_type().is_symlink());
        assert!(system_under_test.is_ok());
        assert_eq!(
            fs::read_to_string(root.path().join("Link.md")).unwrap(),
            "- [ ] Milk\n"
        );
        assert_eq!(fs::read_to_string(&target).unwrap(), "- [ ] Milk\n");
    }
}

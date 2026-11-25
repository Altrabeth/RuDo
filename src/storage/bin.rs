//! The bin: a folder inside the workspace that receives deleted lists and folders, so that a
//! deletion can be undone.
//!
//! The bin is a plain folder, `.trash/` by default, the folder Obsidian itself uses for deleted
//! notes. An entry keeps its path relative to the workspace root inside the bin
//! (`Home/Groceries.md` becomes `.trash/Home/Groceries.md`), and moving it there is a rename on
//! the same file system, so it is atomic and keeps the identity of the file. The scan and the
//! live reload ignore the bin, wherever it is configured.
//!
//! A small index in the bin, [`INDEX_FILE_NAME`], remembers the original path and the
//! deletion time of every entry RuDo moved there. The folder is the truth and the index only
//! adds to it: [`scan_bin`] lists whatever is in the folder, also entries the index does not
//! know (such as notes Obsidian deleted), and ignores records of entries that are gone. Only
//! [`delete_from_bin`] and [`empty_bin`] delete files for good.

use std::collections::{HashMap, HashSet};
use std::ffi::{OsStr, OsString};
use std::fs;
use std::io::{self, ErrorKind, Write};
use std::path::{Component, Path, PathBuf};

use jiff::Timestamp;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::storage::entries::{EntryError, classify, delete_folder, delete_list, move_to};
use crate::storage::workspace::{Entry, relocate_entry, scan_tree};

/// The name of the index file in the bin, which no scan shows because it is not a list.
pub const INDEX_FILE_NAME: &str = ".rudo-bin.json";

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

/// A list file or folder in the bin with the path it is restored to: where RuDo moved it from,
/// or a folder the user chose for an entry that another program put into the bin.
///
/// The index of the bin stores one per entry RuDo moved there.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TrashedEntry {
    original: PathBuf,
    location: PathBuf,
    /// `None` for an entry that RuDo did not move into the bin.
    deleted_at: Option<Timestamp>,
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

    /// Returns when RuDo moved the entry into the bin, `None` if another program did.
    #[must_use]
    pub fn deleted_at(&self) -> Option<Timestamp> {
        self.deleted_at
    }
}

/// A list or folder in the bin, loaded like an entry of the workspace, with what the index
/// remembers about it.
#[derive(Debug)]
pub struct BinEntry {
    /// The entry with paths relative to the bin.
    entry: Entry,
    /// `None` if the index does not know the entry, for example a note deleted by Obsidian.
    trashed: Option<TrashedEntry>,
}

impl BinEntry {
    /// Returns the list or folder with its loaded documents; its paths are relative to the
    /// bin.
    #[must_use]
    pub fn entry(&self) -> &Entry {
        &self.entry
    }

    /// Returns the path of the entry relative to the bin.
    #[must_use]
    pub fn location(&self) -> &Path {
        self.entry.path()
    }

    /// Returns the path the entry had before RuDo moved it into the bin, relative to the
    /// workspace root, or `None` if it is unknown.
    #[must_use]
    pub fn original(&self) -> Option<&Path> {
        self.trashed.as_ref().map(TrashedEntry::original)
    }

    /// Returns when RuDo moved the entry into the bin, or `None` if it is unknown.
    #[must_use]
    pub fn deleted_at(&self) -> Option<Timestamp> {
        self.trashed.as_ref().and_then(TrashedEntry::deleted_at)
    }

    /// Returns the entry to restore to its original path, or `None` if it is unknown.
    #[must_use]
    pub fn trashed(&self) -> Option<&TrashedEntry> {
        self.trashed.as_ref()
    }

    /// Returns the entry to restore into `folder`, relative to the workspace root, keeping its
    /// name; for entries whose original path is unknown.
    #[must_use]
    pub fn restored_into(&self, folder: &Path) -> TrashedEntry {
        let location = self.location().to_path_buf();
        TrashedEntry {
            original: folder.join(location.file_name().unwrap_or_default()),
            location,
            deleted_at: self.deleted_at(),
        }
    }

    /// Moves the entry, which was just moved into the bin as `trashed`, from the workspace
    /// tree into the bin.
    pub(crate) fn moved_in(mut entry: Entry, trashed: TrashedEntry) -> Self {
        relocate_entry(&mut entry, trashed.location.clone());
        Self {
            entry,
            trashed: Some(trashed),
        }
    }
}

/// Lists the bin of the workspace `root`: every list and folder in it as a scan of the
/// workspace would load it, with what the index remembers, the most recently deleted first and
/// entries the index does not know last.
///
/// A folder of the bin that only holds entries RuDo moved there, such as `Home` of
/// `.trash/Home/Groceries.md`, is not an entry itself; everything else the index does not
/// know is one. A missing bin is empty; a bin that cannot be read is logged and treated as
/// empty, so it never stops RuDo from starting.
#[must_use]
pub fn scan_bin(root: &Path, bin: &BinPath) -> Vec<BinEntry> {
    let bin_folder = root.join(&bin.0);
    let tree = match scan_tree(&bin_folder, bin) {
        Ok(tree) => tree,
        Err(error) if error.kind() == ErrorKind::NotFound => return Vec::new(),
        Err(error) => {
            tracing::warn!(bin = %bin_folder.display(), %error, "cannot read the bin");
            return Vec::new();
        }
    };
    let mut records: HashMap<PathBuf, TrashedEntry> = read_index(&bin_folder)
        .into_iter()
        .map(|record| (record.location.clone(), record))
        .collect();
    let containers: HashSet<PathBuf> = records
        .keys()
        .flat_map(|location| location.ancestors().skip(1))
        .map(Path::to_path_buf)
        .collect();
    let mut entries = Vec::new();
    collect_bin_entries(tree, &mut records, &containers, &mut entries);
    entries.sort_by(|first, second| {
        second
            .deleted_at()
            .cmp(&first.deleted_at())
            .then_with(|| first.location().cmp(second.location()))
    });
    entries
}

/// Moves the entries of the bin tree `tree` into `entries`: those the index `records` knows
/// with their record, the folders in `containers` that only hold such entries by their
/// content, and everything else as an entry the index does not know.
fn collect_bin_entries(
    tree: Vec<Entry>,
    records: &mut HashMap<PathBuf, TrashedEntry>,
    containers: &HashSet<PathBuf>,
    entries: &mut Vec<BinEntry>,
) {
    for entry in tree {
        let trashed = records.remove(entry.path());
        match entry {
            Entry::Folder(folder) if trashed.is_none() && containers.contains(folder.path()) => {
                collect_bin_entries(folder.into_entries(), records, containers, entries);
            }
            entry @ (Entry::Folder(_) | Entry::List(_)) => {
                entries.push(BinEntry { entry, trashed });
            }
        }
    }
}

/// Moves the list file or folder at `path`, relative to the workspace `root`, into the bin and
/// returns where it went.
///
/// The entry keeps its path relative to the root inside the bin; the folders it needs there
/// are created. If an entry with that name is already in the bin, the moved one is numbered
/// (`Groceries 2.md`, `Work 2`), so nothing in the bin is ever replaced. A symlink is moved
/// itself, not the file it points to. The original path and the time are added to the index of
/// the bin; if the index cannot be written, the entry is in the bin all the same and the
/// failure is logged.
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
                let trashed = TrashedEntry {
                    original: path.to_path_buf(),
                    location,
                    deleted_at: Some(Timestamp::now()),
                };
                update_index(&bin_folder, |records| records.push(trashed.clone()));
                return Ok(trashed);
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
/// Folders on the way to the original path that no longer exist are created, folders in the
/// bin that the entry leaves empty are removed, and the entry leaves the index of the bin.
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
    left_bin(&bin_folder, &entry.location);
    Ok(target)
}

/// Deletes the list or folder at `location`, relative to the bin of the workspace `root`,
/// permanently, a folder with everything inside it.
///
/// This and [`empty_bin`] are the only permanent deletions of RuDo, and both delete with
/// [`delete_list`] and [`delete_folder`]. The entry leaves the index, and folders of the bin
/// that it leaves empty are removed. A symlink is deleted itself, not the file it points to.
///
/// # Errors
///
/// Returns [`EntryError::Io`] if the entry is not in the bin or something inside it cannot be
/// deleted; what could be deleted is gone.
pub fn delete_from_bin(root: &Path, bin: &BinPath, location: &Path) -> Result<(), EntryError> {
    let bin_folder = root.join(&bin.0);
    delete_recursively(&bin_folder.join(location))?;
    left_bin(&bin_folder, location);
    Ok(())
}

/// What [`empty_bin`] did: how many entries the bin folder held and why some of them are still
/// there.
#[derive(Debug, Default)]
pub struct EmptiedBin {
    entries: usize,
    failures: Vec<EntryError>,
}

impl EmptiedBin {
    /// Returns the number of files and folders at the top of the bin folder before it was
    /// emptied, the index left out.
    #[must_use]
    pub fn entries(&self) -> usize {
        self.entries
    }

    /// Returns why entries could not be deleted, one error per entry; empty if the bin is
    /// empty now.
    #[must_use]
    pub fn failures(&self) -> &[EntryError] {
        &self.failures
    }
}

/// Deletes everything in the bin of the workspace `root` permanently, also files that are no
/// lists and entries the index does not know, and returns how many entries there were and
/// which could not be deleted.
///
/// The bin folder itself stays. The index keeps only the entries that are still there, so a
/// bin emptied completely has no index. A missing bin is already empty.
#[must_use]
pub fn empty_bin(root: &Path, bin: &BinPath) -> EmptiedBin {
    let bin_folder = root.join(&bin.0);
    let children = match fs::read_dir(&bin_folder) {
        Ok(children) => children,
        Err(error) if error.kind() == ErrorKind::NotFound => return EmptiedBin::default(),
        Err(error) => {
            return EmptiedBin {
                entries: 1,
                failures: vec![classify(bin_folder, error)],
            };
        }
    };
    let mut emptied = EmptiedBin::default();
    for child in children {
        let result = child
            .map_err(|error| classify(bin_folder.clone(), error))
            .and_then(|child| {
                let path = child.path();
                if child.file_name() == INDEX_FILE_NAME {
                    return Ok(false);
                }
                delete_recursively(&path).map(|()| true)
            });
        match result {
            Ok(false) => {}
            Ok(true) => emptied.entries += 1,
            Err(error) => {
                emptied.entries += 1;
                emptied.failures.push(error);
            }
        }
    }
    update_index(&bin_folder, |records| {
        records.retain(|record| fs::symlink_metadata(bin_folder.join(&record.location)).is_ok());
    });
    emptied
}

/// Deletes the file, symlink or folder at `path`, a folder with everything inside it, with the
/// permanent deletions of [`crate::storage::entries`].
fn delete_recursively(path: &Path) -> Result<(), EntryError> {
    let metadata = fs::symlink_metadata(path).map_err(|error| classify(path.into(), error))?;
    if !metadata.is_dir() {
        return delete_list(path);
    }
    for child in fs::read_dir(path).map_err(|error| classify(path.into(), error))? {
        let child = child.map_err(|error| classify(path.into(), error))?;
        delete_recursively(&child.path())?;
    }
    delete_folder(path)
}

/// Forgets the entry at `location`, and everything inside it, that just left the bin folder
/// `bin_folder`: removes it from the index and removes the folders it leaves empty.
fn left_bin(bin_folder: &Path, location: &Path) {
    update_index(bin_folder, |records| {
        records.retain(|record| !record.location.starts_with(location));
    });
    remove_empty_folders(bin_folder, location);
}

/// Reads the index of the bin folder `bin_folder`; a missing index is empty, and an index that
/// cannot be read or parsed is logged and treated as empty, because the folder is the truth.
fn read_index(bin_folder: &Path) -> Vec<TrashedEntry> {
    let path = bin_folder.join(INDEX_FILE_NAME);
    let bytes = match fs::read(&path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == ErrorKind::NotFound => return Vec::new(),
        Err(error) => {
            tracing::warn!(index = %path.display(), %error, "cannot read the index of the bin");
            return Vec::new();
        }
    };
    serde_json::from_slice(&bytes).unwrap_or_else(|error| {
        tracing::warn!(index = %path.display(), %error, "ignoring the corrupt index of the bin");
        Vec::new()
    })
}

/// Applies `change` to the index of the bin folder `bin_folder` and writes it back; a failure
/// is logged, because the entries in the bin are where they are all the same.
fn update_index(bin_folder: &Path, change: impl FnOnce(&mut Vec<TrashedEntry>)) {
    let mut records = read_index(bin_folder);
    change(&mut records);
    if let Err(error) = write_index(bin_folder, &records) {
        tracing::warn!(bin = %bin_folder.display(), %error, "cannot write the index of the bin");
    }
}

/// Replaces the index of the bin folder `bin_folder` atomically by `records`, or removes it if
/// there are none.
fn write_index(bin_folder: &Path, records: &[TrashedEntry]) -> io::Result<()> {
    let path = bin_folder.join(INDEX_FILE_NAME);
    if records.is_empty() {
        return match fs::remove_file(&path) {
            Err(error) if error.kind() != ErrorKind::NotFound => Err(error),
            Ok(()) | Err(_) => Ok(()),
        };
    }
    let mut file = tempfile::NamedTempFile::new_in(bin_folder)?;
    serde_json::to_writer_pretty(&mut file, records)?;
    file.write_all(b"\n")?;
    file.as_file().sync_all()?;
    file.persist(&path).map_err(|error| error.error)?;
    Ok(())
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

    /// Writes the index of the default bin in `root` with `(original, location, deleted_at)`.
    fn write_records(root: &Path, records: &[(&str, &str, &str)]) {
        let records: Vec<TrashedEntry> = records
            .iter()
            .map(|(original, location, deleted_at)| TrashedEntry {
                original: PathBuf::from(original),
                location: PathBuf::from(location),
                deleted_at: Some(deleted_at.parse().unwrap()),
            })
            .collect();
        create(root, &[(".trash/", "")]);
        write_index(&root.join(".trash"), &records).unwrap();
    }

    /// Returns `(location, original)` of every entry `scan_bin` finds in `root`.
    fn scanned(root: &Path) -> Vec<(String, Option<String>)> {
        scan_bin(root, &bin())
            .iter()
            .map(|entry| {
                (
                    entry.location().display().to_string(),
                    entry.original().map(|path| path.display().to_string()),
                )
            })
            .collect()
    }

    #[test]
    fn move_to_bin_records_the_original_path_and_the_time_in_the_index() {
        let root = TempDir::new().unwrap();
        create(root.path(), &[("Home/Groceries.md", "- [ ] Milk\n")]);
        let before = Timestamp::now();

        let system_under_test =
            move_to_bin(root.path(), &bin(), Path::new("Home/Groceries.md")).unwrap();

        let records = read_index(&root.path().join(".trash"));
        assert_eq!(records, std::slice::from_ref(&system_under_test));
        assert_eq!(system_under_test.original(), Path::new("Home/Groceries.md"));
        assert!(
            system_under_test
                .deleted_at()
                .is_some_and(|deleted_at| deleted_at >= before && deleted_at <= Timestamp::now())
        );
    }

    #[test]
    fn restore_removes_the_entry_and_its_contents_from_the_index() {
        let root = TempDir::new().unwrap();
        create(root.path(), &[("Work/Plan.md", ""), ("Inbox.md", "")]);
        let work = move_to_bin(root.path(), &bin(), Path::new("Work")).unwrap();
        let inbox = move_to_bin(root.path(), &bin(), Path::new("Inbox.md")).unwrap();

        let system_under_test = restore(root.path(), &bin(), &work);

        assert!(system_under_test.is_ok());
        assert_eq!(read_index(&root.path().join(".trash")), [inbox]);
    }

    #[test]
    fn restoring_the_last_entry_removes_the_index() {
        let root = TempDir::new().unwrap();
        create(root.path(), &[("Inbox.md", "")]);
        let inbox = move_to_bin(root.path(), &bin(), Path::new("Inbox.md")).unwrap();

        let system_under_test = restore(root.path(), &bin(), &inbox);

        assert!(system_under_test.is_ok());
        assert!(!root.path().join(".trash").join(INDEX_FILE_NAME).exists());
    }

    #[test]
    fn scan_bin_lists_known_entries_newest_first_with_their_original_path() {
        let root = TempDir::new().unwrap();
        create(
            root.path(),
            &[
                (".trash/Home/Groceries 2.md", "- [ ] Milk\n"),
                (".trash/Work/Plan.md", "- [ ] Write\n"),
            ],
        );
        write_records(
            root.path(),
            &[
                (
                    "Home/Groceries.md",
                    "Home/Groceries 2.md",
                    "2026-11-20T12:00:00Z",
                ),
                ("Work", "Work", "2026-11-24T12:00:00Z"),
            ],
        );
        let expected = [
            ("Work".to_owned(), Some("Work".to_owned())),
            (
                "Home/Groceries 2.md".to_owned(),
                Some("Home/Groceries.md".to_owned()),
            ),
        ];

        let system_under_test = scanned(root.path());

        assert_eq!(system_under_test, expected);
    }

    #[test]
    fn scan_bin_loads_the_documents_and_times_of_the_entries() {
        let root = TempDir::new().unwrap();
        create(
            root.path(),
            &[(".trash/Groceries.md", "- [ ] Milk\n- [x] Eggs\n")],
        );
        write_records(
            root.path(),
            &[("Groceries.md", "Groceries.md", "2026-11-20T12:00:00Z")],
        );
        let expected_time: Timestamp = "2026-11-20T12:00:00Z".parse().unwrap();

        let system_under_test = scan_bin(root.path(), &bin());

        let [entry] = &system_under_test[..] else {
            panic!("expected one entry, got {system_under_test:?}");
        };
        assert_eq!(entry.deleted_at(), Some(expected_time));
        let Entry::List(list) = entry.entry() else {
            panic!("expected a list");
        };
        assert_eq!(list.content().unwrap().document().progress().total(), 2);
    }

    #[test]
    fn scan_bin_shows_entries_the_index_does_not_know_last() {
        let root = TempDir::new().unwrap();
        create(
            root.path(),
            &[
                (".trash/Note.md", "Obsidian"),
                (".trash/Home/Groceries.md", ""),
                (".trash/Home/Recipe.md", "Obsidian"),
                (".trash/Old/", ""),
            ],
        );
        write_records(
            root.path(),
            &[(
                "Home/Groceries.md",
                "Home/Groceries.md",
                "2026-11-20T12:00:00Z",
            )],
        );
        let expected = [
            (
                "Home/Groceries.md".to_owned(),
                Some("Home/Groceries.md".to_owned()),
            ),
            ("Home/Recipe.md".to_owned(), None),
            ("Note.md".to_owned(), None),
            ("Old".to_owned(), None),
        ];

        let system_under_test = scanned(root.path());

        assert_eq!(system_under_test, expected);
    }

    #[rstest]
    #[case::corrupt("{ not json")]
    #[case::wrong_shape("{\"entries\": 3}")]
    fn scan_bin_shows_the_entries_of_a_bin_with_a_corrupt_index(#[case] index: &str) {
        let root = TempDir::new().unwrap();
        create(
            root.path(),
            &[
                (".trash/Groceries.md", ""),
                (".trash/.rudo-bin.json", index),
            ],
        );
        let expected = [("Groceries.md".to_owned(), None)];

        let system_under_test = scanned(root.path());

        assert_eq!(system_under_test, expected);
    }

    #[test]
    fn scan_bin_ignores_records_of_entries_removed_by_hand() {
        let root = TempDir::new().unwrap();
        create(root.path(), &[(".trash/Inbox.md", "")]);
        write_records(
            root.path(),
            &[
                ("Gone.md", "Gone.md", "2026-11-20T12:00:00Z"),
                ("Inbox.md", "Inbox.md", "2026-11-21T12:00:00Z"),
            ],
        );
        let expected = [("Inbox.md".to_owned(), Some("Inbox.md".to_owned()))];

        let system_under_test = scanned(root.path());

        assert_eq!(system_under_test, expected);
    }

    #[test]
    fn scan_bin_of_a_missing_bin_is_empty() {
        let root = TempDir::new().unwrap();

        let system_under_test = scan_bin(root.path(), &bin());

        assert!(system_under_test.is_empty());
    }

    #[test]
    fn restored_into_keeps_the_name_in_the_chosen_folder() {
        let root = TempDir::new().unwrap();
        create(root.path(), &[(".trash/Note.md", "")]);
        let entries = scan_bin(root.path(), &bin());
        let expected = Path::new("Home/Note.md");

        let system_under_test = entries[0].restored_into(Path::new("Home"));

        assert_eq!(system_under_test.original(), expected);
        assert_eq!(system_under_test.location(), Path::new("Note.md"));
        assert_eq!(entries[0].trashed(), None);
    }

    #[test]
    fn delete_from_bin_deletes_a_folder_with_everything_inside() {
        let root = TempDir::new().unwrap();
        create(
            root.path(),
            &[
                ("Home/Work/Plan.md", ""),
                ("Home/Work/Old/notes.txt", ""),
                ("Inbox.md", ""),
            ],
        );
        let work = move_to_bin(root.path(), &bin(), Path::new("Home/Work")).unwrap();
        let inbox = move_to_bin(root.path(), &bin(), Path::new("Inbox.md")).unwrap();

        let system_under_test = delete_from_bin(root.path(), &bin(), work.location());

        assert!(system_under_test.is_ok());
        assert!(!root.path().join(".trash/Home").exists());
        assert_eq!(read_index(&root.path().join(".trash")), [inbox]);
    }

    #[test]
    fn delete_from_bin_reports_an_entry_that_is_gone() {
        let root = TempDir::new().unwrap();

        let system_under_test = delete_from_bin(root.path(), &bin(), Path::new("Gone.md"));

        assert!(matches!(system_under_test, Err(EntryError::Io { .. })));
    }

    #[cfg(unix)]
    #[test]
    fn delete_from_bin_deletes_a_symlink_but_not_its_target() {
        let root = TempDir::new().unwrap();
        let outside = TempDir::new().unwrap();
        let target = outside.path().join("Real.md");
        fs::write(&target, "- [ ] Milk\n").unwrap();
        create(root.path(), &[(".trash/", "")]);
        std::os::unix::fs::symlink(&target, root.path().join(".trash/Link.md")).unwrap();

        let system_under_test = delete_from_bin(root.path(), &bin(), Path::new("Link.md"));

        assert!(system_under_test.is_ok());
        assert!(fs::symlink_metadata(root.path().join(".trash/Link.md")).is_err());
        assert!(target.is_file());
    }

    #[test]
    fn empty_bin_deletes_everything_and_the_index() {
        let root = TempDir::new().unwrap();
        create(
            root.path(),
            &[
                ("Home/Groceries.md", ""),
                (".trash/Note.md", "Obsidian"),
                (".trash/image.png", ""),
                (".trash/.hidden/", ""),
            ],
        );
        move_to_bin(root.path(), &bin(), Path::new("Home/Groceries.md")).unwrap();

        let system_under_test = empty_bin(root.path(), &bin());

        assert_eq!(system_under_test.entries(), 4);
        assert!(system_under_test.failures().is_empty());
        assert_eq!(fs::read_dir(root.path().join(".trash")).unwrap().count(), 0);
    }

    #[test]
    fn empty_bin_of_a_missing_bin_has_nothing_to_do() {
        let root = TempDir::new().unwrap();

        let system_under_test = empty_bin(root.path(), &bin());

        assert_eq!(system_under_test.entries(), 0);
        assert!(system_under_test.failures().is_empty());
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

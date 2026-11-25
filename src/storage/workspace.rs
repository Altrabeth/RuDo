//! Scanning the workspace folder into the tree of folders and lists shown in the lists pane.
//!
//! The scan walks the directories sequentially, which only reads directory entries, and then
//! reads and parses the list files in parallel. A list file that cannot be loaded stays in
//! the tree and carries its [`LoadError`], so one broken file never hides the others.

use std::cmp::Ordering;
use std::collections::HashMap;
use std::ffi::OsStr;
use std::fmt::{self, Display, Formatter};
use std::fs::{self, DirEntry};
use std::io;
use std::mem;
use std::path::{Path, PathBuf};
use std::str::{self, Utf8Error};

use rayon::iter::{IntoParallelIterator, ParallelIterator};
use thiserror::Error;
use tracing::warn;

use crate::config::IndentStyle;
use crate::markdown::document::TodoDocument;
use crate::paths::portable;
use crate::storage::bin::{BinEntry, BinPath, TrashedEntry, scan_bin};
use crate::storage::entries::EntryName;
use crate::storage::fingerprint::Fingerprint;

/// File extension of list files, compared case-insensitively.
const LIST_EXTENSION: &str = "md";

/// The workspace folder after a scan: its folders and lists with their loaded documents.
#[derive(Debug)]
pub struct Workspace {
    root: PathBuf,
    bin: BinPath,
    default_indent: IndentStyle,
    entries: Vec<Entry>,
    /// The content of the bin, most recently deleted first; see [`scan_bin`].
    bin_entries: Vec<BinEntry>,
}

impl Workspace {
    /// Scans the folder `root` recursively and loads every list in it.
    ///
    /// Directories become [`Folder`]s and `*.md` files (extension compared case-insensitively)
    /// become [`List`]s; all other files are never opened. Hidden entries, whose name starts
    /// with `.`, are kept and tell so with [`Entry::is_hidden`], but a hidden folder without
    /// lists at any depth is left out, and the bin `bin` is never part of the tree, wherever it
    /// is configured; its content is scanned separately, see [`Workspace::bin_entries`].
    /// Symlinked directories are not followed, so symlink loops cannot make the scan endless. Every directory is sorted folders first, then lists, each
    /// case-insensitively with ties broken by the exact name.
    ///
    /// List files are read and parsed in parallel. A list that cannot be read or is not UTF-8
    /// keeps its place in the tree and carries its [`LoadError`]; a subdirectory that cannot
    /// be read is left out with a warning. `default_indent` is the configured indentation for
    /// lists without indented tasks, see [`TodoDocument::indent_style`].
    ///
    /// # Errors
    ///
    /// Returns [`WorkspaceError::ReadRoot`] if `root` itself cannot be read, including when it
    /// does not exist or is not a directory. Whether a missing root is created is the
    /// caller's decision.
    pub fn scan(
        root: impl Into<PathBuf>,
        bin: BinPath,
        default_indent: IndentStyle,
    ) -> Result<Self, WorkspaceError> {
        let root = root.into();
        let entries = match scan_tree(&root, &bin) {
            Ok(entries) => entries,
            Err(source) => return Err(WorkspaceError::ReadRoot { path: root, source }),
        };
        let bin_entries = scan_bin(&root, &bin);
        Ok(Self {
            root,
            bin,
            default_indent,
            entries,
            bin_entries,
        })
    }

    /// Scans the folder again, as [`Workspace::scan`] does, after other programs created,
    /// removed or renamed folders and lists, and returns what changed among the lists the
    /// workspace held before.
    ///
    /// A list that is still at its path is reloaded as [`Workspace::reload`] does: its
    /// document is kept if the file did not change, so its task ids stay valid. Lists that
    /// are no longer found are returned with their content.
    ///
    /// # Errors
    ///
    /// Returns [`WorkspaceError::ReadRoot`] if the workspace folder cannot be read any more;
    /// the workspace is left as it was.
    pub fn rescan(&mut self) -> Result<Rescan, WorkspaceError> {
        let pending = read_directory(&self.root, &self.bin, Path::new("")).map_err(|source| {
            WorkspaceError::ReadRoot {
                path: self.root.clone(),
                source,
            }
        })?;
        let mut known = HashMap::new();
        take_lists(mem::take(&mut self.entries), &mut known);
        let pending = adopt_known_lists(pending, &mut known);
        let (entries, replaced) = load_entries(&self.root, pending);
        self.entries = entries;
        let mut removed: Vec<List> = known
            .into_iter()
            .map(|(path, mut list)| {
                list.path = path;
                list
            })
            .collect();
        removed.sort_unstable_by(|first, second| first.path.cmp(&second.path));
        Ok(Rescan { replaced, removed })
    }

    /// Returns the workspace folder that was scanned.
    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Returns the bin, the folder that receives deleted lists and folders.
    #[must_use]
    pub fn bin(&self) -> &BinPath {
        &self.bin
    }

    /// Returns the lists and folders in the bin, the most recently deleted first, as the last
    /// scan of the bin found them and RuDo changed them since.
    ///
    /// The bin is scanned with the workspace and again by [`Workspace::rescan_bin`]; a
    /// [`Workspace::rescan`] leaves it alone, because the live reload ignores the bin.
    #[must_use]
    pub fn bin_entries(&self) -> &[BinEntry] {
        &self.bin_entries
    }

    /// Returns the entry of the bin at `location`, relative to the bin, or `None` if the bin
    /// has no such entry.
    #[must_use]
    pub fn bin_entry(&self, location: &Path) -> Option<&BinEntry> {
        self.bin_entries
            .iter()
            .find(|entry| entry.location() == location)
    }

    /// Scans the bin again, for example after emptying it left some entries behind.
    pub fn rescan_bin(&mut self) {
        self.bin_entries = scan_bin(&self.root, &self.bin);
    }

    /// Moves the folder or list that was just moved into the bin as `trashed` from the tree
    /// into the bin, with its loaded documents, and returns `false` if the tree has no entry
    /// at its original path.
    pub fn trash(&mut self, trashed: TrashedEntry) -> bool {
        let Some(entry) = self.remove(trashed.original()) else {
            return false;
        };
        self.bin_entries
            .insert(0, BinEntry::moved_in(entry, trashed));
        true
    }

    /// Removes the entry at `location`, relative to the bin, after it left the bin on disk,
    /// and returns it; `None` if the bin has no such entry.
    pub fn remove_bin_entry(&mut self, location: &Path) -> Option<BinEntry> {
        let index = self
            .bin_entries
            .iter()
            .position(|entry| entry.location() == location)?;
        Some(self.bin_entries.remove(index))
    }

    /// Returns the configured indentation for lists without indented tasks.
    #[must_use]
    pub fn default_indent(&self) -> IndentStyle {
        self.default_indent
    }

    /// Returns the top-level folders and lists in display order.
    #[must_use]
    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    /// Returns the list at `path`, or `None` if the workspace has no such list.
    #[must_use]
    pub fn list(&self, path: &ListPath) -> Option<&List> {
        find_list(&self.entries, path)
    }

    /// Returns the list at `path` for editing, or `None` if the workspace has no such list.
    #[must_use]
    pub fn list_mut(&mut self, path: &ListPath) -> Option<&mut List> {
        find_list_mut(&mut self.entries, path)
    }

    /// Returns the path of the list file at `path` on disk, for reading and writing it.
    #[must_use]
    pub fn absolute_path(&self, path: &ListPath) -> PathBuf {
        self.root.join(&path.0)
    }

    /// Returns the folder or list at `path`, relative to the workspace root, or `None` if the
    /// workspace has no entry there.
    #[must_use]
    pub fn entry(&self, path: &Path) -> Option<&Entry> {
        let siblings = folder_entries(&self.entries, path.parent()?)?;
        siblings.iter().find(|entry| entry.path() == path)
    }

    /// Returns whether `path`, relative to the workspace root, is a folder of the workspace or
    /// the root itself (an empty path).
    #[must_use]
    pub fn contains_folder(&self, path: &Path) -> bool {
        folder_entries(&self.entries, path).is_some()
    }

    /// Reads the list file at `path` again, as a scan would, and replaces its content if the
    /// file changed since RuDo last loaded or saved it, judged by its [`Fingerprint`].
    ///
    /// A file that cannot be loaded any more replaces the content with its [`LoadError`].
    /// Returns `None` if the workspace has no list at `path`.
    pub fn reload(&mut self, path: &ListPath) -> Option<Reload> {
        let file = self.absolute_path(path);
        Some(self.list_mut(path)?.reload(&file))
    }

    /// Adds an empty folder or list named `name` to the folder `parent` in display order and
    /// returns it, after it was created on disk, so that the workspace need not be scanned
    /// again.
    ///
    /// `parent` is relative to the workspace root and empty for the root itself. A list is
    /// added with an empty document, as a scan would load its new, empty file. Returns `None`
    /// if `parent` is not a folder of the workspace.
    pub fn insert(&mut self, kind: EntryKind, parent: &Path, name: &EntryName) -> Option<&Entry> {
        let entries = folder_entries_mut(&mut self.entries, parent)?;
        let entry = match kind {
            EntryKind::Folder => Entry::Folder(Folder {
                name: name.as_str().to_owned(),
                path: parent.join(name.as_str()),
                entries: Vec::new(),
            }),
            EntryKind::List => Entry::List(List {
                name: name.as_str().to_owned(),
                path: ListPath(parent.join(name.list_file_name())),
                content: Ok(LoadedList {
                    document: TodoDocument::parse(""),
                    fingerprint: Fingerprint::of(&[]),
                }),
            }),
        };
        Some(insert_in_display_order(entries, entry))
    }

    /// Renames the folder or list at `path` to `name` and moves it to its place in display
    /// order, after it was renamed on disk, and returns it.
    ///
    /// `path` is relative to the workspace root. The paths of all folders and lists inside a
    /// renamed folder change with it; their documents stay loaded. A list is renamed to
    /// `<name>.md`, as on disk. Returns `None` if the workspace has no entry at `path`.
    pub fn rename(&mut self, path: &Path, name: &EntryName) -> Option<&Entry> {
        let file_name = match self.entry(path)?.kind() {
            EntryKind::Folder => name.as_str().to_owned(),
            EntryKind::List => name.list_file_name(),
        };
        self.relocate(path, &path.with_file_name(file_name))
    }

    /// Moves the folder or list at `path` into the folder `folder`, keeping its name, after it
    /// was moved on disk, and returns it at its new place in display order.
    ///
    /// Both paths are relative to the workspace root; an empty `folder` is the root itself. As
    /// with [`Workspace::rename`], everything inside a moved folder moves with it and stays
    /// loaded. Returns `None` if the workspace has no entry at `path`, if `folder` is not a
    /// folder of the workspace, or if it is `path` itself or inside it.
    pub fn move_into(&mut self, path: &Path, folder: &Path) -> Option<&Entry> {
        let file_name = path.file_name()?;
        self.relocate(path, &folder.join(file_name))
    }

    /// Moves the folder or list at `from` to the path `to`, which names its folder and its new
    /// file name, after it was moved on disk, and returns it at its new place in display order.
    ///
    /// Both paths are relative to the workspace root. The entry is named after the new file
    /// name as a scan would name it, and everything inside a moved folder moves with it and
    /// stays loaded. Returns `None` if the workspace has no entry at `from`, or if the folder
    /// of `to` is not a folder of the workspace or lies inside `from`.
    ///
    /// # Panics
    ///
    /// Never panics in practice: the target folder is checked to exist and to lie outside the
    /// moved entry before the entry is taken out of the tree.
    pub fn relocate(&mut self, from: &Path, to: &Path) -> Option<&Entry> {
        let (folder, file_name) = (to.parent()?, to.file_name()?);
        if folder.starts_with(from) || folder_entries(&self.entries, folder).is_none() {
            return None;
        }
        let (entries, index) = entry_position(&mut self.entries, from)?;
        let mut entry = entries.remove(index);
        match &mut entry {
            Entry::Folder(moved) => moved.name = file_name.to_string_lossy().into_owned(),
            Entry::List(moved) => {
                moved.name = to
                    .file_stem()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned();
            }
        }
        relocate_entry(&mut entry, to.to_path_buf());
        let target = folder_entries_mut(&mut self.entries, folder)
            .expect("the target folder exists and is not inside the moved entry");
        Some(insert_in_display_order(target, entry))
    }

    /// Removes the folder or list at `path` with everything inside it, after it was deleted
    /// on disk, and returns it.
    ///
    /// `path` is relative to the workspace root. Returns `None` if the workspace has no entry
    /// at `path`.
    pub fn remove(&mut self, path: &Path) -> Option<Entry> {
        let (entries, index) = entry_position(&mut self.entries, path)?;
        Some(entries.remove(index))
    }
}

/// Returns the entries of the folder that contains the entry at `path` and its index there.
fn entry_position<'workspace>(
    entries: &'workspace mut Vec<Entry>,
    path: &Path,
) -> Option<(&'workspace mut Vec<Entry>, usize)> {
    let siblings = folder_entries_mut(entries, path.parent()?)?;
    let index = siblings.iter().position(|entry| entry.path() == path)?;
    Some((siblings, index))
}

/// Inserts `entry` into `entries`, which are in display order, and returns it.
fn insert_in_display_order(entries: &mut Vec<Entry>, entry: Entry) -> &Entry {
    let index = entries.partition_point(|existing| {
        display_order(
            (existing.kind(), existing.name()),
            (entry.kind(), entry.name()),
        ) == Ordering::Less
    });
    entries.insert(index, entry);
    &entries[index]
}

/// Reads the folder `root` recursively into its tree of folders and lists, without the bin
/// `bin`, and loads every list, as [`Workspace::scan`] does.
pub(crate) fn scan_tree(root: &Path, bin: &BinPath) -> io::Result<Vec<Entry>> {
    let pending = read_directory(root, bin, Path::new(""))?;
    Ok(load_entries(root, pending).0)
}

/// Moves `entry` to the relative path `to`, together with everything inside it.
pub(crate) fn relocate_entry(entry: &mut Entry, to: PathBuf) {
    match entry {
        Entry::Folder(folder) => {
            for child in &mut folder.entries {
                let file_name = child
                    .path()
                    .file_name()
                    .expect("the path of an entry ends with its file name");
                let child_to = to.join(file_name);
                relocate_entry(child, child_to);
            }
            folder.path = to;
        }
        Entry::List(list) => list.path = ListPath(to),
    }
}

/// Returns where `path` is after the entry at `from` was moved to `to`, or `None` if `path` is
/// neither `from` nor inside it. All paths are relative to the workspace root.
pub(crate) fn moved_path(path: &Path, from: &Path, to: &Path) -> Option<PathBuf> {
    let inside = path.strip_prefix(from).ok()?;
    if inside.as_os_str().is_empty() {
        Some(to.to_path_buf())
    } else {
        Some(to.join(inside))
    }
}

/// A list's file path relative to the workspace root, which identifies the list.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ListPath(PathBuf);

impl ListPath {
    /// Returns the path relative to the workspace root.
    #[must_use]
    pub fn as_path(&self) -> &Path {
        &self.0
    }

    /// Returns the path of this list after the folder or list at `from` was renamed to `to`,
    /// or `None` if the list is neither `from` nor inside it.
    pub(crate) fn moved(&self, from: &Path, to: &Path) -> Option<Self> {
        moved_path(&self.0, from, to).map(Self)
    }
}

impl Display for ListPath {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        portable(&self.0).fmt(formatter)
    }
}

/// One entry of the workspace tree.
#[derive(Debug)]
pub enum Entry {
    /// A directory with its own entries.
    Folder(Folder),
    /// A list file.
    List(List),
}

impl Entry {
    /// Returns the name shown in the lists pane.
    #[must_use]
    pub fn name(&self) -> &str {
        match self {
            Self::Folder(folder) => folder.name(),
            Self::List(list) => list.name(),
        }
    }

    /// Returns the path of the directory or list file relative to the workspace root.
    #[must_use]
    pub fn path(&self) -> &Path {
        match self {
            Self::Folder(folder) => folder.path(),
            Self::List(list) => list.path().as_path(),
        }
    }

    /// Returns whether this is a folder or a list.
    #[must_use]
    pub fn kind(&self) -> EntryKind {
        match self {
            Self::Folder(_) => EntryKind::Folder,
            Self::List(_) => EntryKind::List,
        }
    }

    /// Returns whether the entry is hidden: its name or the name of a folder containing it
    /// starts with `.`, as in `.obsidian/` or `.drafts.md`.
    #[must_use]
    pub fn is_hidden(&self) -> bool {
        is_hidden_path(self.path())
    }
}

/// A directory of the workspace.
#[derive(Debug)]
pub struct Folder {
    name: String,
    path: PathBuf,
    entries: Vec<Entry>,
}

impl Folder {
    /// Returns the directory name, with invalid UTF-8 replaced.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Returns the directory path relative to the workspace root.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Returns the folders and lists inside this folder in display order.
    #[must_use]
    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    /// Returns the folders and lists inside this folder, consuming it.
    pub(crate) fn into_entries(self) -> Vec<Entry> {
        self.entries
    }
}

/// A list file of the workspace and the result of loading it.
#[derive(Debug)]
pub struct List {
    name: String,
    path: ListPath,
    content: Result<LoadedList, LoadError>,
}

impl List {
    /// Returns the file name without extension, with invalid UTF-8 replaced.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Returns the path that identifies this list.
    #[must_use]
    pub fn path(&self) -> &ListPath {
        &self.path
    }

    /// Returns the loaded document and fingerprint.
    ///
    /// # Errors
    ///
    /// Returns the [`LoadError`] if the file could not be loaded during the scan.
    pub fn content(&self) -> Result<&LoadedList, &LoadError> {
        self.content.as_ref()
    }

    /// Returns the loaded document and fingerprint for editing.
    ///
    /// # Errors
    ///
    /// Returns the [`LoadError`] if the file could not be loaded during the scan.
    pub fn content_mut(&mut self) -> Result<&mut LoadedList, &LoadError> {
        self.content.as_mut().map_err(|error| &*error)
    }

    /// Loads the list file at `file` again if its content differs from the loaded one.
    fn reload(&mut self, file: &Path) -> Reload {
        let bytes = match fs::read(file) {
            Ok(bytes) => bytes,
            Err(error) => {
                self.content = Err(LoadError::Read(error));
                return Reload::Replaced;
            }
        };
        let fingerprint = Fingerprint::of(&bytes);
        let previous = self.content.as_ref().ok();
        if previous.is_some_and(|loaded| loaded.fingerprint == fingerprint) {
            return Reload::Unchanged;
        }
        self.content = parse_list(&bytes, fingerprint, previous.map(LoadedList::document));
        Reload::Replaced
    }
}

/// What [`Workspace::rescan`] changed among the lists the workspace held before.
#[derive(Debug, Default)]
pub struct Rescan {
    replaced: Vec<ListPath>,
    removed: Vec<List>,
}

impl Rescan {
    /// Returns the lists still in the workspace whose content was replaced because their file
    /// changed or cannot be loaded any more.
    #[must_use]
    pub fn replaced(&self) -> &[ListPath] {
        &self.replaced
    }

    /// Returns the lists that are no longer in the workspace, ordered by path.
    #[must_use]
    pub fn removed(&self) -> &[List] {
        &self.removed
    }
}

/// Whether [`Workspace::reload`] replaced the content of a list.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Reload {
    /// The file still has the content RuDo last loaded or saved, which was kept.
    Unchanged,
    /// The file changed or cannot be loaded any more, and its content was replaced.
    Replaced,
}

/// The parsed document of a list and the fingerprint of the bytes it was parsed from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoadedList {
    document: TodoDocument,
    fingerprint: Fingerprint,
}

impl LoadedList {
    /// Returns the document.
    #[must_use]
    pub fn document(&self) -> &TodoDocument {
        &self.document
    }

    /// Returns the document for editing.
    #[must_use]
    pub fn document_mut(&mut self) -> &mut TodoDocument {
        &mut self.document
    }

    /// Returns the fingerprint of the file content RuDo last loaded or saved.
    #[must_use]
    pub fn fingerprint(&self) -> Fingerprint {
        self.fingerprint
    }

    /// Records the fingerprint of content RuDo has just written or reloaded.
    pub fn set_fingerprint(&mut self, fingerprint: Fingerprint) {
        self.fingerprint = fingerprint;
    }
}

/// Why a list file could not be loaded.
#[derive(Debug, Error)]
pub enum LoadError {
    /// The file could not be read.
    #[error("cannot read the file")]
    Read(#[source] io::Error),
    /// The file content is not valid UTF-8.
    #[error("the file is not valid UTF-8")]
    NotUtf8(#[source] Utf8Error),
}

/// An error that prevents scanning the workspace.
#[derive(Debug, Error)]
pub enum WorkspaceError {
    /// The workspace folder itself cannot be read.
    #[error("cannot read workspace folder {}", path.display())]
    ReadRoot {
        /// The workspace folder.
        path: PathBuf,
        /// The underlying error.
        #[source]
        source: io::Error,
    },
}

/// Whether an [`Entry`] is a folder or a list; folders are shown before lists.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, strum::EnumIter)]
pub enum EntryKind {
    /// A directory.
    Folder,
    /// A list file.
    List,
}

/// An entry found by the directory walk whose list file has not been loaded yet.
enum PendingEntry {
    Folder {
        name: String,
        path: PathBuf,
        entries: Vec<PendingEntry>,
    },
    List {
        name: String,
        path: ListPath,
    },
    /// A list the workspace held before a [`Workspace::rescan`], still at its path.
    Known(List),
}

impl PendingEntry {
    fn kind(&self) -> EntryKind {
        match self {
            Self::Folder { .. } => EntryKind::Folder,
            Self::List { .. } | Self::Known(_) => EntryKind::List,
        }
    }

    fn name(&self) -> &str {
        match self {
            Self::Folder { name, .. } | Self::List { name, .. } => name,
            Self::Known(list) => list.name(),
        }
    }
}

/// Reads the directory `relative` below `root` recursively, without the bin `bin`, sorted in
/// display order.
fn read_directory(root: &Path, bin: &BinPath, relative: &Path) -> io::Result<Vec<PendingEntry>> {
    let mut entries: Vec<PendingEntry> = fs::read_dir(root.join(relative))?
        .filter_map(|entry| match entry {
            Ok(entry) => classify(root, bin, relative, &entry),
            Err(error) => {
                warn!(directory = %relative.display(), %error, "skipping unreadable entry");
                None
            }
        })
        .collect();
    entries.sort_by(|first, second| {
        display_order((first.kind(), first.name()), (second.kind(), second.name()))
    });
    Ok(entries)
}

/// Orders entries by kind and name: folders first, then lists, each by [`compare_names`].
fn display_order(first: (EntryKind, &str), second: (EntryKind, &str)) -> Ordering {
    first
        .0
        .cmp(&second.0)
        .then_with(|| compare_names(first.1, second.1))
}

/// Turns a directory entry into a folder or list, or `None` if it is not part of the tree.
fn classify(root: &Path, bin: &BinPath, relative: &Path, entry: &DirEntry) -> Option<PendingEntry> {
    let file_name = entry.file_name();
    let path = relative.join(&file_name);
    if bin.contains(&path) {
        return None;
    }
    let file_type = match entry.file_type() {
        Ok(file_type) => file_type,
        Err(error) => {
            warn!(path = %entry.path().display(), %error, "skipping entry of unknown type");
            return None;
        }
    };
    if file_type.is_dir() {
        return read_folder(root, bin, path, &file_name);
    }
    // A symlink to a list file is a list, but a symlinked directory is never followed.
    if !has_list_extension(&path) || (file_type.is_symlink() && entry.path().is_dir()) {
        return None;
    }
    Some(PendingEntry::List {
        name: path
            .file_stem()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned(),
        path: ListPath(path),
    })
}

/// Reads the folder at `path`, or logs a warning and returns `None` if it is unreadable.
///
/// A hidden folder without lists at any depth is left out too, so that folders such as
/// `.git/` never clutter the tree; its subfolders are left out first, from the bottom up.
fn read_folder(
    root: &Path,
    bin: &BinPath,
    path: PathBuf,
    file_name: &OsStr,
) -> Option<PendingEntry> {
    match read_directory(root, bin, &path) {
        Ok(entries) if entries.is_empty() && is_hidden_path(&path) => None,
        Ok(entries) => Some(PendingEntry::Folder {
            name: file_name.to_string_lossy().into_owned(),
            path,
            entries,
        }),
        Err(error) => {
            warn!(directory = %path.display(), %error, "skipping unreadable directory");
            None
        }
    }
}

/// Returns whether `file_name` starts with `.`, which makes it a hidden entry.
fn is_hidden(file_name: &OsStr) -> bool {
    file_name.as_encoded_bytes().first() == Some(&b'.')
}

/// Returns whether the entry at `path`, relative to the workspace root, is hidden: its name or
/// the name of a folder containing it starts with `.`.
pub(crate) fn is_hidden_path(path: &Path) -> bool {
    path.components()
        .any(|component| is_hidden(component.as_os_str()))
}

/// Returns whether `path` has the extension of list files.
pub(crate) fn has_list_extension(path: &Path) -> bool {
    path.extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case(LIST_EXTENSION))
}

/// Orders names case-insensitively, breaking ties by the exact name, without allocating.
fn compare_names(first: &str, second: &str) -> Ordering {
    lowercase_chars(first)
        .cmp(lowercase_chars(second))
        .then_with(|| first.cmp(second))
}

fn lowercase_chars(name: &str) -> impl Iterator<Item = char> + '_ {
    name.chars().flat_map(char::to_lowercase)
}

/// Moves every list of `entries` into `lists`, keyed by its path.
///
/// The path moves into the key instead of being cloned; the list gets it back when it leaves
/// the map, so a list in the map has an empty path.
fn take_lists(entries: Vec<Entry>, lists: &mut HashMap<ListPath, List>) {
    for entry in entries {
        match entry {
            Entry::Folder(folder) => take_lists(folder.entries, lists),
            Entry::List(mut list) => {
                let path = mem::replace(&mut list.path, ListPath(PathBuf::new()));
                lists.insert(path, list);
            }
        }
    }
}

/// Replaces every pending list that is in `known` by the known list, taking it out of
/// `known`.
fn adopt_known_lists(
    pending: Vec<PendingEntry>,
    known: &mut HashMap<ListPath, List>,
) -> Vec<PendingEntry> {
    pending
        .into_iter()
        .map(|entry| match entry {
            PendingEntry::Folder {
                name,
                path,
                entries,
            } => PendingEntry::Folder {
                name,
                path,
                entries: adopt_known_lists(entries, known),
            },
            PendingEntry::List { name, path } => match known.remove(&path) {
                Some(mut list) => {
                    list.path = path;
                    PendingEntry::Known(list)
                }
                None => PendingEntry::List { name, path },
            },
            known_list @ PendingEntry::Known(_) => known_list,
        })
        .collect()
}

/// Loads the list files of `pending` in parallel, keeping the order of the entries, and
/// returns the entries with the paths of the known lists whose content was replaced.
fn load_entries(root: &Path, pending: Vec<PendingEntry>) -> (Vec<Entry>, Vec<ListPath>) {
    let (entries, replaced): (Vec<Entry>, Vec<Vec<ListPath>>) = pending
        .into_par_iter()
        .map(|entry| match entry {
            PendingEntry::Folder {
                name,
                path,
                entries,
            } => {
                let (entries, replaced) = load_entries(root, entries);
                let folder = Entry::Folder(Folder {
                    name,
                    path,
                    entries,
                });
                (folder, replaced)
            }
            PendingEntry::List { name, path } => {
                let content = load_list(&root.join(&path.0));
                let list = Entry::List(List {
                    name,
                    path,
                    content,
                });
                (list, Vec::new())
            }
            PendingEntry::Known(mut list) => {
                let replaced = match list.reload(&root.join(&list.path.0)) {
                    Reload::Replaced => vec![list.path.clone()],
                    Reload::Unchanged => Vec::new(),
                };
                (Entry::List(list), replaced)
            }
        })
        .unzip();
    (entries, replaced.into_iter().flatten().collect())
}

fn load_list(path: &Path) -> Result<LoadedList, LoadError> {
    let bytes = fs::read(path).map_err(LoadError::Read)?;
    parse_list(&bytes, Fingerprint::of(&bytes), None)
}

/// Parses the content `bytes` of a list file, whose fingerprint is `fingerprint`, as a new
/// version of `previous` if the list was loaded before.
fn parse_list(
    bytes: &[u8],
    fingerprint: Fingerprint,
    previous: Option<&TodoDocument>,
) -> Result<LoadedList, LoadError> {
    let text = str::from_utf8(bytes).map_err(LoadError::NotUtf8)?;
    let document = match previous {
        Some(previous) => previous.reparse(text),
        None => TodoDocument::parse(text),
    };
    Ok(LoadedList {
        document,
        fingerprint,
    })
}

fn find_list<'workspace>(
    entries: &'workspace [Entry],
    path: &ListPath,
) -> Option<&'workspace List> {
    entries.iter().find_map(|entry| match entry {
        Entry::List(list) => (list.path == *path).then_some(list),
        Entry::Folder(folder) if path.0.starts_with(&folder.path) => {
            find_list(&folder.entries, path)
        }
        Entry::Folder(_) => None,
    })
}

/// Returns the entries of the folder at `path`, or of the root if `path` is empty.
fn folder_entries<'workspace>(
    entries: &'workspace [Entry],
    path: &Path,
) -> Option<&'workspace [Entry]> {
    if path.as_os_str().is_empty() {
        return Some(entries);
    }
    entries.iter().find_map(|entry| match entry {
        Entry::Folder(folder) if path.starts_with(&folder.path) => {
            if folder.path == path {
                Some(folder.entries.as_slice())
            } else {
                folder_entries(&folder.entries, path)
            }
        }
        Entry::Folder(_) | Entry::List(_) => None,
    })
}

/// Returns the entries of the folder at `path`, or of the root if `path` is empty.
fn folder_entries_mut<'workspace>(
    entries: &'workspace mut Vec<Entry>,
    path: &Path,
) -> Option<&'workspace mut Vec<Entry>> {
    if path.as_os_str().is_empty() {
        return Some(entries);
    }
    entries.iter_mut().find_map(|entry| match entry {
        Entry::Folder(folder) if path.starts_with(&folder.path) => {
            if folder.path == path {
                Some(&mut folder.entries)
            } else {
                folder_entries_mut(&mut folder.entries, path)
            }
        }
        Entry::Folder(_) | Entry::List(_) => None,
    })
}

fn find_list_mut<'workspace>(
    entries: &'workspace mut [Entry],
    path: &ListPath,
) -> Option<&'workspace mut List> {
    entries.iter_mut().find_map(|entry| match entry {
        Entry::List(list) => (list.path == *path).then_some(list),
        Entry::Folder(folder) if path.0.starts_with(&folder.path) => {
            find_list_mut(&mut folder.entries, path)
        }
        Entry::Folder(_) => None,
    })
}

#[cfg(test)]
mod tests {
    use std::io::ErrorKind;

    use rstest::rstest;
    use strum::IntoEnumIterator;
    use tempfile::TempDir;

    use super::*;
    use crate::markdown::document::TaskId;
    use crate::storage::bin::{BinPath, move_to_bin};

    fn write_file(root: &Path, relative: &str, content: &[u8]) {
        let path = root.join(relative);
        fs::create_dir_all(path.parent().expect("a file has a parent")).unwrap();
        fs::write(path, content).unwrap();
    }

    fn create_dir(root: &Path, relative: &str) {
        fs::create_dir_all(root.join(relative)).unwrap();
    }

    fn scan(root: &TempDir) -> Workspace {
        Workspace::scan(root.path(), BinPath::default(), IndentStyle::default()).unwrap()
    }

    fn list_path(relative: &str) -> ListPath {
        ListPath(PathBuf::from(relative))
    }

    /// Flattens the tree into relative paths in display order, folders with a trailing `/`.
    fn outline(entries: &[Entry]) -> Vec<String> {
        entries
            .iter()
            .flat_map(|entry| match entry {
                Entry::Folder(folder) => {
                    let mut lines = vec![format!("{}/", portable(folder.path()))];
                    lines.extend(outline(folder.entries()));
                    lines
                }
                Entry::List(list) => vec![list.path().to_string()],
            })
            .collect()
    }

    /// Flattens the tree into display names in display order.
    fn names(entries: &[Entry]) -> Vec<&str> {
        entries
            .iter()
            .flat_map(|entry| {
                let children = match entry {
                    Entry::Folder(folder) => names(folder.entries()),
                    Entry::List(_) => Vec::new(),
                };
                std::iter::once(entry.name()).chain(children)
            })
            .collect()
    }

    #[test]
    fn scan_builds_nested_folders_and_lists() {
        let root = TempDir::new().unwrap();
        write_file(root.path(), "inbox.md", b"- [ ] a\n");
        write_file(root.path(), "projects/home.md", b"- [ ] b\n");
        write_file(root.path(), "projects/work/today.md", b"- [ ] c\n");
        let expected = ["projects/", "projects/work/", "projects/work/today.md"]
            .into_iter()
            .chain(["projects/home.md", "inbox.md"])
            .map(String::from)
            .collect::<Vec<_>>();

        let system_under_test = scan(&root);

        assert_eq!(outline(system_under_test.entries()), expected);
    }

    #[test]
    fn scan_keeps_hidden_entries_in_display_order() {
        let root = TempDir::new().unwrap();
        write_file(root.path(), ".obsidian/templates/daily.md", b"");
        write_file(root.path(), ".hidden.md", b"");
        write_file(root.path(), "visible.md", b"");
        let expected = [
            ".obsidian/",
            ".obsidian/templates/",
            ".obsidian/templates/daily.md",
            ".hidden.md",
            "visible.md",
        ]
        .map(String::from);

        let system_under_test = scan(&root);

        assert_eq!(outline(system_under_test.entries()), expected);
    }

    #[rstest]
    #[case::hidden_list(".hidden.md", true)]
    #[case::hidden_folder(".obsidian", true)]
    #[case::inside_a_hidden_folder(".obsidian/templates/daily.md", true)]
    #[case::visible_list("visible.md", false)]
    #[case::visible_folder("home", false)]
    #[case::hidden_inside_a_visible_folder("home/.drafts.md", true)]
    fn entries_inside_or_named_with_a_dot_are_hidden(#[case] path: &str, #[case] expected: bool) {
        let root = TempDir::new().unwrap();
        write_file(root.path(), ".obsidian/templates/daily.md", b"");
        write_file(root.path(), ".hidden.md", b"");
        write_file(root.path(), "visible.md", b"");
        write_file(root.path(), "home/.drafts.md", b"");
        let workspace = scan(&root);

        let system_under_test = workspace.entry(Path::new(path)).unwrap();

        assert_eq!(system_under_test.is_hidden(), expected);
    }

    #[test]
    fn scan_leaves_out_hidden_folders_without_lists_at_any_depth() {
        let root = TempDir::new().unwrap();
        write_file(root.path(), ".git/objects/ab/cdef", b"");
        write_file(root.path(), ".obsidian/plugins/tasks/main.js", b"");
        write_file(root.path(), ".obsidian/notes/todo.md", b"");
        create_dir(root.path(), ".empty");
        create_dir(root.path(), "visible");
        let expected = [
            ".obsidian/",
            ".obsidian/notes/",
            ".obsidian/notes/todo.md",
            "visible/",
        ]
        .map(String::from);

        let system_under_test = scan(&root);

        assert_eq!(outline(system_under_test.entries()), expected);
    }

    #[test]
    fn scan_never_includes_the_bin() {
        let root = TempDir::new().unwrap();
        write_file(root.path(), ".trash/deleted.md", b"");
        write_file(root.path(), "home/.trash/kept.md", b"");
        let expected = ["home/", "home/.trash/", "home/.trash/kept.md"].map(String::from);

        let system_under_test = scan(&root);

        assert_eq!(outline(system_under_test.entries()), expected);
    }

    #[test]
    fn scan_loads_the_content_of_the_bin_separately() {
        let root = TempDir::new().unwrap();
        write_file(root.path(), ".trash/deleted.md", b"- [ ] Gone\n");
        write_file(root.path(), "inbox.md", b"");
        let expected = Path::new("deleted.md");

        let system_under_test = scan(&root);

        let locations: Vec<&Path> = system_under_test
            .bin_entries()
            .iter()
            .map(BinEntry::location)
            .collect();
        assert_eq!(locations, [expected]);
        assert!(system_under_test.bin_entry(expected).is_some());
        assert!(system_under_test.bin_entry(Path::new("inbox.md")).is_none());
    }

    #[test]
    fn trash_moves_an_entry_with_its_documents_from_the_tree_into_the_bin() {
        let root = TempDir::new().unwrap();
        write_file(root.path(), "home/chores.md", b"- [ ] Vacuum\n");
        let mut workspace = scan(&root);
        let trashed = move_to_bin(root.path(), workspace.bin(), Path::new("home")).unwrap();
        let expected = Path::new("home/chores.md");

        let system_under_test = workspace.trash(trashed);

        assert!(system_under_test);
        assert!(workspace.entries().is_empty());
        let Entry::Folder(folder) = workspace.bin_entries()[0].entry() else {
            panic!("expected the folder in the bin");
        };
        let Entry::List(list) = &folder.entries()[0] else {
            panic!("expected the list inside the folder");
        };
        assert_eq!(list.path().as_path(), expected);
        assert_eq!(list.content().unwrap().document().progress().total(), 1);
    }

    #[test]
    fn trash_of_an_entry_missing_from_the_tree_changes_nothing() {
        let root = TempDir::new().unwrap();
        write_file(root.path(), "inbox.md", b"");
        let trashed = move_to_bin(root.path(), &BinPath::default(), Path::new("inbox.md")).unwrap();
        let mut workspace = scan(&root);
        let bin_entries = workspace.bin_entries().len();

        let system_under_test = workspace.trash(trashed);

        assert!(!system_under_test);
        assert_eq!(workspace.bin_entries().len(), bin_entries);
    }

    #[test]
    fn remove_bin_entry_forgets_it_and_rescan_bin_finds_it_again() {
        let root = TempDir::new().unwrap();
        write_file(root.path(), ".trash/deleted.md", b"");
        let mut workspace = scan(&root);
        let removed = workspace
            .remove_bin_entry(Path::new("deleted.md"))
            .map(|entry| entry.location().to_path_buf());
        let after_removal = workspace.bin_entries().len();

        workspace.rescan_bin();

        assert_eq!(removed, Some(PathBuf::from("deleted.md")));
        assert_eq!(after_removal, 0);
        assert_eq!(workspace.bin_entries().len(), 1);
    }

    #[rstest]
    #[case::folder("home", true)]
    #[case::root("", true)]
    #[case::hidden_folder(".notes", true)]
    #[case::list("inbox.md", false)]
    #[case::missing("garden", false)]
    fn contains_folder_finds_folders_and_the_root(#[case] path: &str, #[case] expected: bool) {
        let root = TempDir::new().unwrap();
        write_file(root.path(), "home/chores.md", b"");
        write_file(root.path(), ".notes/ideas.md", b"");
        write_file(root.path(), "inbox.md", b"");
        let workspace = scan(&root);

        let system_under_test = workspace.contains_folder(Path::new(path));

        assert_eq!(system_under_test, expected);
    }

    #[rstest]
    #[case::top_level("Bin")]
    #[case::nested("Archive/Bin")]
    fn scan_skips_the_bin_even_if_its_name_is_not_hidden(#[case] bin: &str) {
        let root = TempDir::new().unwrap();
        write_file(root.path(), &format!("{bin}/deleted.md"), b"");
        write_file(root.path(), "Archive/kept.md", b"");
        write_file(root.path(), "Other/Bin/kept.md", b"");
        let bin = BinPath::try_from(PathBuf::from(bin)).unwrap();
        let expected = vec![
            "Archive/".to_owned(),
            "Archive/kept.md".to_owned(),
            "Other/".to_owned(),
            "Other/Bin/".to_owned(),
            "Other/Bin/kept.md".to_owned(),
        ];

        let system_under_test = Workspace::scan(root.path(), bin, IndentStyle::default()).unwrap();

        assert_eq!(outline(system_under_test.entries()), expected);
    }

    #[test]
    fn scan_only_includes_markdown_files_case_insensitively() {
        let root = TempDir::new().unwrap();
        write_file(root.path(), "notes.txt", b"");
        write_file(root.path(), "image.png", b"");
        write_file(root.path(), "README", b"");
        write_file(root.path(), "lower.md", b"");
        write_file(root.path(), "upper.MD", b"");
        write_file(root.path(), "mixed.Md", b"");
        let expected = vec![
            "lower.md".to_owned(),
            "mixed.Md".to_owned(),
            "upper.MD".to_owned(),
        ];

        let system_under_test = scan(&root);

        assert_eq!(outline(system_under_test.entries()), expected);
    }

    #[test]
    fn reload_of_an_unchanged_file_keeps_the_document() {
        let root = TempDir::new().unwrap();
        write_file(root.path(), "inbox.md", b"- [ ] a\n");
        let mut workspace = scan(&root);
        let path = list_path("inbox.md");
        let expected = workspace.list(&path).unwrap().content().unwrap().clone();

        let system_under_test = workspace.reload(&path);

        assert_eq!(system_under_test, Some(Reload::Unchanged));
        assert_eq!(workspace.list(&path).unwrap().content().unwrap(), &expected);
    }

    #[test]
    fn reload_of_a_changed_file_replaces_document_and_fingerprint() {
        let root = TempDir::new().unwrap();
        write_file(root.path(), "inbox.md", b"- [ ] a\n");
        let mut workspace = scan(&root);
        let path = list_path("inbox.md");
        let content = b"- [x] a\n- [ ] b\n";
        write_file(root.path(), "inbox.md", content);

        let system_under_test = workspace.reload(&path);

        assert_eq!(system_under_test, Some(Reload::Replaced));
        let loaded = workspace.list(&path).unwrap().content().unwrap();
        assert_eq!(loaded.document().to_string(), "- [x] a\n- [ ] b\n");
        assert_eq!(loaded.fingerprint(), Fingerprint::of(content));
    }

    #[rstest]
    #[case::not_utf8(Some(&[0xff][..]))]
    #[case::deleted(None)]
    fn reload_of_a_file_that_cannot_be_loaded_keeps_its_error(#[case] content: Option<&[u8]>) {
        let root = TempDir::new().unwrap();
        write_file(root.path(), "inbox.md", b"- [ ] a\n");
        let mut workspace = scan(&root);
        let path = list_path("inbox.md");
        let file = root.path().join("inbox.md");
        match content {
            Some(content) => fs::write(&file, content).unwrap(),
            None => fs::remove_file(&file).unwrap(),
        }

        let system_under_test = workspace.reload(&path);

        assert_eq!(system_under_test, Some(Reload::Replaced));
        assert!(workspace.list(&path).unwrap().content().is_err());
    }

    #[test]
    fn reload_gives_the_new_document_ids_the_old_one_never_used() {
        let root = TempDir::new().unwrap();
        write_file(root.path(), "inbox.md", b"- [ ] a\n");
        let mut workspace = scan(&root);
        let path = list_path("inbox.md");
        let old_ids = task_ids(&workspace, &path);
        write_file(root.path(), "inbox.md", b"- [ ] b\n");
        let _: Option<Reload> = workspace.reload(&path);

        let system_under_test = task_ids(&workspace, &path);

        assert!(system_under_test.iter().all(|id| !old_ids.contains(id)));
    }

    /// Returns the ids of the tasks of the loaded list at `path`.
    fn task_ids(workspace: &Workspace, path: &ListPath) -> Vec<TaskId> {
        let loaded = workspace.list(path).unwrap().content().unwrap();
        loaded.document().tasks().map(|node| node.id()).collect()
    }

    #[test]
    fn rescan_keeps_the_document_of_an_unchanged_list() {
        let root = TempDir::new().unwrap();
        write_file(root.path(), "inbox.md", b"- [ ] a\n");
        let mut workspace = scan(&root);
        let path = list_path("inbox.md");
        let expected = workspace.list(&path).unwrap().content().unwrap().clone();
        write_file(root.path(), "new.md", b"");

        let system_under_test = workspace.rescan().unwrap();

        assert_eq!(system_under_test.replaced(), [] as [ListPath; 0]);
        assert_eq!(system_under_test.removed().len(), 0);
        assert_eq!(workspace.list(&path).unwrap().content().unwrap(), &expected);
        assert!(workspace.list(&list_path("new.md")).is_some());
    }

    #[test]
    fn rescan_reloads_a_changed_list_with_new_ids() {
        let root = TempDir::new().unwrap();
        write_file(root.path(), "home/chores.md", b"- [ ] sweep\n");
        let mut workspace = scan(&root);
        let path = list_path("home/chores.md");
        let old_ids = task_ids(&workspace, &path);
        write_file(root.path(), "home/chores.md", b"- [ ] mop\n");

        let system_under_test = workspace.rescan().unwrap();

        assert_eq!(system_under_test.replaced(), std::slice::from_ref(&path));
        let loaded = workspace.list(&path).unwrap().content().unwrap();
        assert_eq!(loaded.document().to_string(), "- [ ] mop\n");
        assert!(
            task_ids(&workspace, &path)
                .iter()
                .all(|id| !old_ids.contains(id))
        );
    }

    #[test]
    fn rescan_returns_the_removed_lists_in_path_order() {
        let root = TempDir::new().unwrap();
        write_file(root.path(), "inbox.md", b"");
        write_file(root.path(), "home/b.md", b"");
        write_file(root.path(), "home/a.md", b"");
        let mut workspace = scan(&root);
        fs::remove_dir_all(root.path().join("home")).unwrap();
        let expected = [list_path("home/a.md"), list_path("home/b.md")];

        let system_under_test = workspace.rescan().unwrap();

        assert_eq!(
            system_under_test
                .removed()
                .iter()
                .map(List::path)
                .collect::<Vec<_>>(),
            expected.iter().collect::<Vec<_>>()
        );
        assert_eq!(outline(workspace.entries()), ["inbox.md"]);
    }

    #[test]
    fn rescan_of_an_unreadable_root_keeps_the_workspace() {
        let parent = TempDir::new().unwrap();
        let root = parent.path().join("todo");
        write_file(&root, "inbox.md", b"");
        let mut workspace =
            Workspace::scan(&root, BinPath::default(), IndentStyle::default()).unwrap();
        fs::remove_dir_all(&root).unwrap();

        let system_under_test = workspace.rescan();

        assert!(matches!(
            system_under_test,
            Err(WorkspaceError::ReadRoot { .. })
        ));
        assert_eq!(outline(workspace.entries()), ["inbox.md"]);
    }

    #[test]
    fn reload_of_an_unknown_list_does_nothing() {
        let root = TempDir::new().unwrap();
        let mut workspace = scan(&root);
        let path = list_path("missing.md");

        let system_under_test = workspace.reload(&path);

        assert_eq!(system_under_test, None);
    }

    #[test]
    fn scan_keeps_empty_folders() {
        let root = TempDir::new().unwrap();
        create_dir(root.path(), "empty/nested");
        let expected = vec!["empty/".to_owned(), "empty/nested/".to_owned()];

        let system_under_test = scan(&root);

        assert_eq!(outline(system_under_test.entries()), expected);
    }

    #[test]
    fn scan_sorts_folders_first_then_case_insensitively_with_exact_tie_break() {
        let root = TempDir::new().unwrap();
        for list in ["b.md", "a.md", "C.md", "A.md"] {
            write_file(root.path(), list, b"");
        }
        for folder in ["z", "Y"] {
            create_dir(root.path(), folder);
        }
        let expected = ["Y/", "z/", "A.md", "a.md", "b.md", "C.md"]
            .map(String::from)
            .to_vec();

        let system_under_test = scan(&root);

        assert_eq!(outline(system_under_test.entries()), expected);
    }

    #[test]
    fn scan_names_entries_after_file_stem_and_directory_name() {
        let root = TempDir::new().unwrap();
        write_file(root.path(), "Some Folder/Groceries.md", b"");
        let expected = vec!["Some Folder", "Groceries"];

        let system_under_test = scan(&root);

        assert_eq!(names(system_under_test.entries()), expected);
    }

    #[test]
    fn scan_parses_and_fingerprints_every_list() {
        let root = TempDir::new().unwrap();
        let content = "# Groceries\n- [ ] Buy milk\n\t- [x] Check lactose-free\n";
        write_file(root.path(), "folder/groceries.md", content.as_bytes());
        let path = list_path("folder/groceries.md");
        let expected_document = TodoDocument::parse(content);
        let expected_fingerprint = Fingerprint::of(content.as_bytes());

        let system_under_test = scan(&root);

        let loaded = system_under_test.list(&path).unwrap().content().unwrap();
        assert_eq!(loaded.document(), &expected_document);
        assert_eq!(loaded.fingerprint(), expected_fingerprint);
    }

    #[test]
    fn scan_keeps_a_list_that_is_not_utf8_with_its_error() {
        let root = TempDir::new().unwrap();
        write_file(root.path(), "broken.md", b"- [ ] \xff\xfe\n");
        write_file(root.path(), "fine.md", b"- [ ] a\n");
        let broken = list_path("broken.md");
        let fine = list_path("fine.md");

        let system_under_test = scan(&root);

        assert!(matches!(
            system_under_test.list(&broken).unwrap().content(),
            Err(LoadError::NotUtf8(_))
        ));
        assert!(system_under_test.list(&fine).unwrap().content().is_ok());
    }

    #[test]
    fn scan_reports_a_missing_root() {
        let root = TempDir::new().unwrap();
        let missing = root.path().join("missing");

        let system_under_test =
            Workspace::scan(&missing, BinPath::default(), IndentStyle::default());

        assert!(matches!(
            system_under_test,
            Err(WorkspaceError::ReadRoot { path, source })
                if path == missing && source.kind() == ErrorKind::NotFound
        ));
    }

    #[test]
    fn scan_reports_a_root_that_is_a_file() {
        let root = TempDir::new().unwrap();
        write_file(root.path(), "file.md", b"");
        let file = root.path().join("file.md");

        let system_under_test = Workspace::scan(&file, BinPath::default(), IndentStyle::default());

        assert!(matches!(
            system_under_test,
            Err(WorkspaceError::ReadRoot { .. })
        ));
    }

    #[test]
    fn scan_remembers_root_and_default_indent() {
        let root = TempDir::new().unwrap();
        let indent = IndentStyle::Spaces(crate::config::IndentWidth::new(2).unwrap());

        let system_under_test = Workspace::scan(root.path(), BinPath::default(), indent).unwrap();

        assert_eq!(system_under_test.root(), root.path());
        assert_eq!(system_under_test.default_indent(), indent);
    }

    #[rstest]
    #[case::top_level("inbox.md", true)]
    #[case::nested("projects/work/today.md", true)]
    #[case::folder("projects", false)]
    #[case::missing("projects/missing.md", false)]
    #[case::other_folder("other/today.md", false)]
    fn list_finds_lists_by_path(#[case] relative: &str, #[case] expected: bool) {
        let root = TempDir::new().unwrap();
        write_file(root.path(), "inbox.md", b"");
        write_file(root.path(), "projects/work/today.md", b"");
        create_dir(root.path(), "other");
        let path = list_path(relative);

        let system_under_test = scan(&root);

        assert_eq!(
            system_under_test.list(&path).map(List::path),
            expected.then_some(&path)
        );
    }

    #[rstest]
    #[case::top_level_list("inbox.md", Some(EntryKind::List))]
    #[case::nested_list("projects/work/today.md", Some(EntryKind::List))]
    #[case::folder("projects/work", Some(EntryKind::Folder))]
    #[case::missing("projects/missing.md", None)]
    #[case::inside_a_list("inbox.md/today.md", None)]
    #[case::root("", None)]
    fn entry_finds_folders_and_lists_by_path(
        #[case] relative: &str,
        #[case] expected: Option<EntryKind>,
    ) {
        let root = TempDir::new().unwrap();
        write_file(root.path(), "inbox.md", b"");
        write_file(root.path(), "projects/work/today.md", b"");
        let path = Path::new(relative);

        let system_under_test = scan(&root);

        assert_eq!(system_under_test.entry(path).map(Entry::kind), expected);
        assert!(
            system_under_test
                .entry(path)
                .is_none_or(|entry| entry.path() == path)
        );
    }

    #[test]
    fn list_mut_edits_the_document_and_fingerprint_in_place() {
        let root = TempDir::new().unwrap();
        write_file(root.path(), "folder/list.md", b"- [ ] a\n");
        let path = list_path("folder/list.md");
        let new_document = TodoDocument::parse("- [x] a\n");
        let new_fingerprint = Fingerprint::of(b"- [x] a\n");
        let mut workspace = scan(&root);
        let loaded = workspace.list_mut(&path).unwrap().content_mut().unwrap();
        *loaded.document_mut() = new_document.clone();
        loaded.set_fingerprint(new_fingerprint);

        let system_under_test = workspace.list(&path).unwrap().content().unwrap();

        assert_eq!(system_under_test.document(), &new_document);
        assert_eq!(system_under_test.fingerprint(), new_fingerprint);
    }

    #[test]
    fn list_mut_returns_none_for_an_unknown_path() {
        let root = TempDir::new().unwrap();
        let path = list_path("missing.md");
        let mut workspace = scan(&root);

        let system_under_test = workspace.list_mut(&path);

        assert!(system_under_test.is_none());
    }

    #[test]
    fn absolute_path_joins_the_root() {
        let root = TempDir::new().unwrap();
        let path = list_path("folder/list.md");
        let expected = root.path().join("folder").join("list.md");

        let system_under_test = scan(&root);

        assert_eq!(system_under_test.absolute_path(&path), expected);
    }

    #[rstest]
    #[case::case_insensitive("apple", "Banana", Ordering::Less)]
    #[case::uppercase_first_on_tie("A", "a", Ordering::Less)]
    #[case::equal("same", "same", Ordering::Equal)]
    #[case::non_ascii("Äpfel", "äpfel", Ordering::Less)]
    #[case::prefix("list", "List 2", Ordering::Less)]
    fn compare_names_orders_case_insensitively(
        #[case] first: &str,
        #[case] second: &str,
        #[case] expected: Ordering,
    ) {
        let system_under_test = compare_names(first, second);

        assert_eq!(system_under_test, expected);
    }

    fn entry_name(text: &str) -> EntryName {
        EntryName::try_from(text).unwrap()
    }

    const TREE_BEFORE_INSERT: [&str; 5] =
        ["home/", "home/garden/", "home/kitchen.md", "A.md", "c.md"];

    #[rstest]
    #[case::list_at_the_top_level(EntryKind::List, "", "b", 4)]
    #[case::list_after_the_same_name_in_upper_case(EntryKind::List, "", "a", 4)]
    #[case::folder_before_the_lists(EntryKind::Folder, "", "z", 3)]
    #[case::list_in_a_nested_folder(EntryKind::List, "home/garden", "Roses", 2)]
    #[case::folder_before_the_other_folders(EntryKind::Folder, "home", "attic", 1)]
    fn insert_adds_the_entry_in_display_order(
        #[case] kind: EntryKind,
        #[case] parent: &str,
        #[case] name: &str,
        #[case] expected_row: usize,
    ) {
        let root = TempDir::new().unwrap();
        create_dir(root.path(), "home/garden");
        write_file(root.path(), "home/kitchen.md", b"");
        write_file(root.path(), "c.md", b"");
        write_file(root.path(), "A.md", b"");
        let mut workspace = scan(&root);
        let path = Path::new(parent).join(name);
        let inserted_line = match kind {
            EntryKind::Folder => format!("{}/", portable(&path)),
            EntryKind::List => format!("{}.md", portable(&path)),
        };
        let mut expected: Vec<String> = TREE_BEFORE_INSERT.map(String::from).to_vec();
        expected.insert(expected_row, inserted_line);

        let system_under_test = workspace.insert(kind, Path::new(parent), &entry_name(name));

        assert_eq!(system_under_test.map(Entry::name), Some(name));
        assert_eq!(outline(workspace.entries()), expected);
    }

    #[test]
    fn inserted_list_has_an_empty_document() {
        let root = TempDir::new().unwrap();
        let mut workspace = scan(&root);
        let expected = LoadedList {
            document: TodoDocument::parse(""),
            fingerprint: Fingerprint::of(b""),
        };

        let system_under_test = workspace.insert(EntryKind::List, Path::new(""), &entry_name("a"));

        let Some(Entry::List(list)) = system_under_test else {
            panic!("expected a list, got {system_under_test:?}");
        };
        assert_eq!(list.content().unwrap(), &expected);
    }

    #[test]
    fn insert_into_a_missing_folder_adds_nothing() {
        let root = TempDir::new().unwrap();
        write_file(root.path(), "missing.md", b"");
        let mut workspace = scan(&root);
        let expected = vec!["missing.md".to_owned()];

        let system_under_test: Vec<bool> = EntryKind::iter()
            .map(|kind| {
                workspace
                    .insert(kind, Path::new("missing"), &entry_name("a"))
                    .is_some()
            })
            .collect();

        assert_eq!(system_under_test, [false, false]);
        assert_eq!(outline(workspace.entries()), expected);
    }

    #[rstest]
    #[case::list_moves_into_display_order("c.md", "0", &["home/", "home/garden/", "home/kitchen.md", "0.md", "A.md"])]
    #[case::nested_list("home/kitchen.md", "pantry", &["home/", "home/garden/", "home/pantry.md", "A.md", "c.md"])]
    #[case::folder_moves_its_entries("home", "house", &["house/", "house/garden/", "house/kitchen.md", "A.md", "c.md"])]
    fn rename_moves_the_entry_and_everything_inside(
        #[case] path: &str,
        #[case] name: &str,
        #[case] expected: &[&str],
    ) {
        let root = TempDir::new().unwrap();
        create_dir(root.path(), "home/garden");
        write_file(root.path(), "home/kitchen.md", b"- [ ] a\n");
        write_file(root.path(), "c.md", b"");
        write_file(root.path(), "A.md", b"");
        let mut workspace = scan(&root);

        let system_under_test = workspace.rename(Path::new(path), &entry_name(name));

        assert_eq!(system_under_test.map(Entry::name), Some(name));
        assert_eq!(outline(workspace.entries()), expected);
    }

    #[test]
    fn renamed_folder_keeps_the_documents_inside() {
        let root = TempDir::new().unwrap();
        write_file(root.path(), "home/kitchen.md", b"- [ ] a\n");
        let mut workspace = scan(&root);
        let _: Option<&Entry> = workspace.rename(Path::new("home"), &entry_name("house"));
        let expected = TodoDocument::parse("- [ ] a\n");

        let system_under_test = workspace.list(&list_path("house/kitchen.md"));

        assert_eq!(
            system_under_test.unwrap().content().unwrap().document(),
            &expected
        );
    }

    #[rstest]
    #[case::list_into_a_folder("c.md", "home", &["home/", "home/garden/", "home/garden/roses.md", "home/c.md", "home/kitchen.md", "A.md"])]
    #[case::list_into_the_root("home/kitchen.md", "", &["home/", "home/garden/", "home/garden/roses.md", "A.md", "c.md", "kitchen.md"])]
    #[case::folder_with_its_entries("home/garden", "", &["garden/", "garden/roses.md", "home/", "home/kitchen.md", "A.md", "c.md"])]
    fn move_into_moves_the_entry_and_everything_inside(
        #[case] path: &str,
        #[case] folder: &str,
        #[case] expected: &[&str],
    ) {
        let root = TempDir::new().unwrap();
        write_file(root.path(), "home/garden/roses.md", b"- [ ] a\n");
        write_file(root.path(), "home/kitchen.md", b"- [ ] a\n");
        write_file(root.path(), "c.md", b"");
        write_file(root.path(), "A.md", b"");
        let mut workspace = scan(&root);
        let expected_path = Path::new(folder).join(Path::new(path).file_name().unwrap());

        let system_under_test = workspace.move_into(Path::new(path), Path::new(folder));

        assert_eq!(
            system_under_test.map(Entry::path),
            Some(expected_path.as_path())
        );
        assert_eq!(outline(workspace.entries()), expected);
    }

    #[rstest]
    #[case::list("home/kitchen.md", "c/pantry.md", "pantry", &["c/", "c/pantry.md", "home/"])]
    #[case::folder("home", "c/house", "house", &["c/", "c/house/", "c/house/kitchen.md"])]
    fn relocate_moves_and_renames_the_entry_at_once(
        #[case] from: &str,
        #[case] to: &str,
        #[case] expected_name: &str,
        #[case] expected: &[&str],
    ) {
        let root = TempDir::new().unwrap();
        write_file(root.path(), "home/kitchen.md", b"- [ ] a\n");
        create_dir(root.path(), "c");
        let mut workspace = scan(&root);

        let system_under_test = workspace
            .relocate(Path::new(from), Path::new(to))
            .map(Entry::name);

        assert_eq!(system_under_test, Some(expected_name));
        assert_eq!(outline(workspace.entries()), expected);
    }

    #[rstest]
    #[case::missing_entry("missing.md", "home")]
    #[case::missing_folder("c.md", "missing")]
    #[case::folder_into_itself("home", "home")]
    #[case::folder_into_a_descendant("home", "home/garden")]
    fn move_into_without_a_valid_target_changes_nothing(#[case] path: &str, #[case] folder: &str) {
        let root = TempDir::new().unwrap();
        create_dir(root.path(), "home/garden");
        write_file(root.path(), "c.md", b"");
        let mut workspace = scan(&root);
        let expected = vec![
            "home/".to_owned(),
            "home/garden/".to_owned(),
            "c.md".to_owned(),
        ];

        let system_under_test = workspace
            .move_into(Path::new(path), Path::new(folder))
            .is_some();

        assert!(!system_under_test);
        assert_eq!(outline(workspace.entries()), expected);
    }

    #[test]
    fn remove_takes_the_entry_out_of_its_folder() {
        let root = TempDir::new().unwrap();
        write_file(root.path(), "home/kitchen.md", b"");
        write_file(root.path(), "home/garden.md", b"");
        let mut workspace = scan(&root);
        let expected = vec!["home/".to_owned(), "home/garden.md".to_owned()];

        let system_under_test = workspace.remove(Path::new("home/kitchen.md"));

        assert_eq!(
            system_under_test.as_ref().map(Entry::path),
            Some(Path::new("home/kitchen.md"))
        );
        assert_eq!(outline(workspace.entries()), expected);
    }

    #[rstest]
    #[case::missing_entry("home/missing.md")]
    #[case::missing_folder("missing/kitchen.md")]
    #[case::empty_path("")]
    fn rename_and_remove_of_a_missing_entry_change_nothing(#[case] path: &str) {
        let root = TempDir::new().unwrap();
        write_file(root.path(), "home/kitchen.md", b"");
        let mut workspace = scan(&root);
        let expected = vec!["home/".to_owned(), "home/kitchen.md".to_owned()];

        let system_under_test = (
            workspace
                .rename(Path::new(path), &entry_name("a"))
                .is_some(),
            workspace.remove(Path::new(path)).is_some(),
        );

        assert_eq!(system_under_test, (false, false));
        assert_eq!(outline(workspace.entries()), expected);
    }

    #[rstest]
    #[case::the_entry_itself("home", Some("house"))]
    #[case::inside("home/garden/roses.md", Some("house/garden/roses.md"))]
    #[case::same_prefix_but_other_name("homework.md", None)]
    #[case::elsewhere("work/home.md", None)]
    fn moved_path_replaces_the_moved_prefix(#[case] path: &str, #[case] expected: Option<&str>) {
        let expected = expected.map(PathBuf::from);

        let system_under_test = moved_path(Path::new(path), Path::new("home"), Path::new("house"));

        assert_eq!(system_under_test, expected);
    }

    #[test]
    fn list_path_moves_with_its_folder() {
        let path = list_path("home/kitchen.md");
        let expected = list_path("house/kitchen.md");

        let system_under_test = path.moved(Path::new("home"), Path::new("house"));

        assert_eq!(system_under_test, Some(expected));
    }

    #[cfg(unix)]
    mod unix {
        use std::os::unix::fs::{PermissionsExt, symlink};

        use super::*;

        /// Removes all permissions from a directory and restores them on drop, so that the
        /// temporary directory can be deleted.
        struct LockedDirectory(PathBuf);

        impl LockedDirectory {
            fn new(path: PathBuf) -> Self {
                fs::set_permissions(&path, fs::Permissions::from_mode(0o000)).unwrap();
                Self(path)
            }
        }

        impl Drop for LockedDirectory {
            fn drop(&mut self) {
                fs::set_permissions(&self.0, fs::Permissions::from_mode(0o755)).unwrap();
            }
        }

        #[test]
        fn scan_does_not_follow_symlinked_directories() {
            let root = TempDir::new().unwrap();
            write_file(root.path(), "real/list.md", b"");
            symlink(root.path(), root.path().join("loop")).unwrap();
            symlink(root.path().join("real"), root.path().join("linked.md")).unwrap();
            let expected = vec!["real/".to_owned(), "real/list.md".to_owned()];

            let system_under_test = scan(&root);

            assert_eq!(outline(system_under_test.entries()), expected);
        }

        #[test]
        fn scan_loads_symlinked_list_files() {
            let root = TempDir::new().unwrap();
            let target = TempDir::new().unwrap();
            write_file(target.path(), "target.md", b"- [ ] a\n");
            symlink(target.path().join("target.md"), root.path().join("link.md")).unwrap();
            let path = list_path("link.md");

            let system_under_test = scan(&root);

            assert!(system_under_test.list(&path).unwrap().content().is_ok());
        }

        #[test]
        fn scan_keeps_an_unreadable_list_with_its_error() {
            let root = TempDir::new().unwrap();
            symlink(
                root.path().join("missing.md"),
                root.path().join("broken.md"),
            )
            .unwrap();
            let path = list_path("broken.md");

            let system_under_test = scan(&root);

            assert!(matches!(
                system_under_test.list(&path).unwrap().content(),
                Err(LoadError::Read(_))
            ));
        }

        #[test]
        fn scan_skips_unreadable_directories() {
            let root = TempDir::new().unwrap();
            write_file(root.path(), "locked/list.md", b"");
            write_file(root.path(), "open.md", b"");
            let _locked = LockedDirectory::new(root.path().join("locked"));
            let expected = vec!["open.md".to_owned()];

            let system_under_test = scan(&root);

            assert_eq!(outline(system_under_test.entries()), expected);
        }
    }
}

//! Scanning the workspace folder into the tree of folders and lists shown in the lists pane.
//!
//! The scan walks the directories sequentially, which only reads directory entries, and then
//! reads and parses the list files in parallel. A list file that cannot be loaded stays in
//! the tree and carries its [`LoadError`], so one broken file never hides the others.

use std::cmp::Ordering;
use std::ffi::OsStr;
use std::fmt::{self, Display, Formatter};
use std::fs::{self, DirEntry};
use std::io;
use std::path::{Path, PathBuf};
use std::str::{self, Utf8Error};

use rayon::iter::{IntoParallelIterator, ParallelIterator};
use thiserror::Error;
use tracing::warn;

use crate::config::IndentStyle;
use crate::markdown::document::TodoDocument;
use crate::storage::fingerprint::Fingerprint;

/// File extension of list files, compared case-insensitively.
const LIST_EXTENSION: &str = "md";

/// The workspace folder after a scan: its folders and lists with their loaded documents.
#[derive(Debug)]
pub struct Workspace {
    root: PathBuf,
    default_indent: IndentStyle,
    entries: Vec<Entry>,
}

impl Workspace {
    /// Scans the folder `root` recursively and loads every list in it.
    ///
    /// Directories become [`Folder`]s and `*.md` files (extension compared case-insensitively)
    /// become [`List`]s; all other files are never opened. Entries whose name starts with `.`
    /// are skipped, and symlinked directories are not followed, so symlink loops cannot make
    /// the scan endless. Every directory is sorted folders first, then lists, each
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
        default_indent: IndentStyle,
    ) -> Result<Self, WorkspaceError> {
        let root = root.into();
        let pending = match read_directory(&root, Path::new("")) {
            Ok(pending) => pending,
            Err(source) => return Err(WorkspaceError::ReadRoot { path: root, source }),
        };
        let entries = load_entries(&root, pending);
        Ok(Self {
            root,
            default_indent,
            entries,
        })
    }

    /// Returns the workspace folder that was scanned.
    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
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
}

impl Display for ListPath {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        self.0.display().fmt(formatter)
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

/// Display order of entry kinds: folders before lists.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum EntryKind {
    Folder,
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
}

impl PendingEntry {
    fn kind(&self) -> EntryKind {
        match self {
            Self::Folder { .. } => EntryKind::Folder,
            Self::List { .. } => EntryKind::List,
        }
    }

    fn name(&self) -> &str {
        match self {
            Self::Folder { name, .. } | Self::List { name, .. } => name,
        }
    }
}

/// Reads the directory `relative` below `root` recursively, sorted in display order.
fn read_directory(root: &Path, relative: &Path) -> io::Result<Vec<PendingEntry>> {
    let mut entries: Vec<PendingEntry> = fs::read_dir(root.join(relative))?
        .filter_map(|entry| match entry {
            Ok(entry) => classify(root, relative, &entry),
            Err(error) => {
                warn!(directory = %relative.display(), %error, "skipping unreadable entry");
                None
            }
        })
        .collect();
    entries.sort_by(|first, second| {
        first
            .kind()
            .cmp(&second.kind())
            .then_with(|| compare_names(first.name(), second.name()))
    });
    Ok(entries)
}

/// Turns a directory entry into a folder or list, or `None` if it is not part of the tree.
fn classify(root: &Path, relative: &Path, entry: &DirEntry) -> Option<PendingEntry> {
    let file_name = entry.file_name();
    if is_hidden(&file_name) {
        return None;
    }
    let file_type = match entry.file_type() {
        Ok(file_type) => file_type,
        Err(error) => {
            warn!(path = %entry.path().display(), %error, "skipping entry of unknown type");
            return None;
        }
    };
    let path = relative.join(&file_name);
    if file_type.is_dir() {
        return read_folder(root, path, &file_name);
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
fn read_folder(root: &Path, path: PathBuf, file_name: &OsStr) -> Option<PendingEntry> {
    match read_directory(root, &path) {
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

fn is_hidden(file_name: &OsStr) -> bool {
    file_name.as_encoded_bytes().first() == Some(&b'.')
}

fn has_list_extension(path: &Path) -> bool {
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

/// Loads the list files of `pending` in parallel, keeping the order of the entries.
fn load_entries(root: &Path, pending: Vec<PendingEntry>) -> Vec<Entry> {
    pending
        .into_par_iter()
        .map(|entry| match entry {
            PendingEntry::Folder {
                name,
                path,
                entries,
            } => Entry::Folder(Folder {
                name,
                path,
                entries: load_entries(root, entries),
            }),
            PendingEntry::List { name, path } => {
                let content = load_list(&root.join(&path.0));
                Entry::List(List {
                    name,
                    path,
                    content,
                })
            }
        })
        .collect()
}

fn load_list(path: &Path) -> Result<LoadedList, LoadError> {
    let bytes = fs::read(path).map_err(LoadError::Read)?;
    let text = str::from_utf8(&bytes).map_err(LoadError::NotUtf8)?;
    Ok(LoadedList {
        document: TodoDocument::parse(text),
        fingerprint: Fingerprint::of(&bytes),
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
    use tempfile::TempDir;

    use super::*;

    fn write_file(root: &Path, relative: &str, content: &[u8]) {
        let path = root.join(relative);
        fs::create_dir_all(path.parent().expect("a file has a parent")).unwrap();
        fs::write(path, content).unwrap();
    }

    fn create_dir(root: &Path, relative: &str) {
        fs::create_dir_all(root.join(relative)).unwrap();
    }

    fn scan(root: &TempDir) -> Workspace {
        Workspace::scan(root.path(), IndentStyle::default()).unwrap()
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
                    let mut lines = vec![format!("{}/", folder.path().display())];
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
    fn scan_skips_hidden_entries() {
        let root = TempDir::new().unwrap();
        write_file(root.path(), ".obsidian/workspace.md", b"");
        write_file(root.path(), ".hidden.md", b"");
        write_file(root.path(), "visible.md", b"");
        let expected = vec!["visible.md".to_owned()];

        let system_under_test = scan(&root);

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

        let system_under_test = Workspace::scan(&missing, IndentStyle::default());

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

        let system_under_test = Workspace::scan(&file, IndentStyle::default());

        assert!(matches!(
            system_under_test,
            Err(WorkspaceError::ReadRoot { .. })
        ));
    }

    #[test]
    fn scan_remembers_root_and_default_indent() {
        let root = TempDir::new().unwrap();
        let indent = IndentStyle::Spaces(crate::config::IndentWidth::new(2).unwrap());

        let system_under_test = Workspace::scan(root.path(), indent).unwrap();

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

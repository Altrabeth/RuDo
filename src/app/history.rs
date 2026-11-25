//! Undo and redo: `History` remembers the documents that edits replaced and the changes of
//! lists and folders on disk.
//!
//! An entry holds the state on the other side of a change. For an edit of a document that is
//! the whole document, with the task that was selected there: undoing swaps it with the
//! document in the workspace and moves the entry to the redo stack, which then holds the state
//! the undo left; redoing is the same swap in the other direction. Keeping one snapshot per
//! change instead of both sides halves the memory and makes undo and redo one operation.
//!
//! A change of lists and folders (creating, renaming, moving or deleting them) is remembered
//! the same way, as the [`EntryOperation`] that reverts it: undoing a deletion is
//! [`EntryOperation::Restore`], and performing it yields the [`EntryOperation::Trash`] that
//! redoes the deletion. These steps touch the disk, so they are performed as an
//! [`Effect::StepEntries`](crate::app::Effect::StepEntries).

use std::collections::VecDeque;
use std::fmt::{self, Display, Formatter};
use std::path::{Path, PathBuf};

use crate::app::lists::EntryChange;
use crate::markdown::document::{TaskId, TodoDocument};
use crate::storage::bin::TrashedEntry;
use crate::storage::workspace::ListPath;

/// How many changes each stack remembers; older ones are forgotten.
const CAPACITY: usize = 100;

/// Which way through the history a step goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, strum::EnumIter)]
pub enum Direction {
    /// Back to the state before the last change.
    Undo,
    /// Forward again to the state the last undo left.
    Redo,
}

impl Direction {
    /// Returns the direction that reverts a step in this one.
    #[must_use]
    pub const fn opposite(self) -> Self {
        match self {
            Self::Undo => Self::Redo,
            Self::Redo => Self::Undo,
        }
    }
}

impl Display for Direction {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Undo => "undo",
            Self::Redo => "redo",
        })
    }
}

/// A change of lists and folders on disk that a step through the history performs.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum EntryOperation {
    /// Move the lists and folders at these paths, relative to the workspace root, into the
    /// bin.
    Trash(Vec<PathBuf>),
    /// Move these lists and folders from the bin back to their original paths.
    Restore {
        /// The entries in the bin.
        entries: Vec<TrashedEntry>,
        /// The list that was active when they were moved into the bin, if it was one of them
        /// or inside one of them; it becomes the active list again.
        active: Option<ListPath>,
    },
    /// Move each list or folder from one path to another.
    Relocate(Vec<Relocation>),
}

impl EntryOperation {
    /// Returns the paths of the entries the operation acts on as the workspace knows them,
    /// relative to the root: where they are, or where they were before they went to the bin.
    #[must_use]
    pub fn workspace_paths(&self) -> Vec<&Path> {
        match self {
            Self::Trash(paths) => paths.iter().map(PathBuf::as_path).collect(),
            Self::Restore { entries, .. } => entries.iter().map(TrashedEntry::original).collect(),
            Self::Relocate(relocations) => relocations.iter().map(Relocation::from).collect(),
        }
    }
}

/// A move of a list or folder from one path to another, both relative to the workspace root.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Relocation {
    from: PathBuf,
    to: PathBuf,
}

impl Relocation {
    /// Creates the move of the entry at `from` to `to`.
    pub(super) fn new(from: PathBuf, to: PathBuf) -> Self {
        Self { from, to }
    }

    /// Returns the path of the entry before the move.
    #[must_use]
    pub fn from(&self) -> &Path {
        &self.from
    }

    /// Returns the path of the entry after the move.
    #[must_use]
    pub fn to(&self) -> &Path {
        &self.to
    }

    /// Returns the move back.
    pub(super) fn reversed(&self) -> Self {
        Self::new(self.to.clone(), self.from.clone())
    }
}

/// The state of a list on the other side of a change: its document and selected task.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct DocumentSnapshot {
    pub(super) list: ListPath,
    pub(super) document: TodoDocument,
    pub(super) selection: Option<TaskId>,
}

/// What a step through the history restores.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum HistoryEntry {
    /// The document of a list, swapped with the one in the workspace.
    Document(DocumentSnapshot),
    /// A change of lists and folders on disk.
    Entries(EntryOperation),
}

/// A stack that forgets its oldest entry when it holds more than [`CAPACITY`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct BoundedStack {
    entries: VecDeque<HistoryEntry>,
}

impl BoundedStack {
    fn push(&mut self, entry: HistoryEntry) {
        if self.entries.len() == CAPACITY {
            self.entries.pop_front();
        }
        self.entries.push_back(entry);
    }

    fn pop(&mut self) -> Option<HistoryEntry> {
        self.entries.pop_back()
    }

    /// Moves the snapshots of the lists at or inside a renamed entry to their new paths.
    ///
    /// Snapshots of removed lists are kept: a list moved to the bin comes back at the same
    /// path when its deletion is undone, and its snapshots are older than the deletion, so no
    /// step reaches them before. Entry operations keep their paths, because every rename by
    /// RuDo is itself in the history and is undone before them.
    fn apply_entry_change(&mut self, change: EntryChange<'_>) {
        let EntryChange::Renamed { from, to } = change else {
            return;
        };
        for entry in &mut self.entries {
            match entry {
                HistoryEntry::Document(snapshot) => {
                    if let Some(moved) = snapshot.list.moved(from, to) {
                        snapshot.list = moved;
                    }
                }
                HistoryEntry::Entries(_) => {}
            }
        }
    }

    fn forget(&mut self, list: &ListPath) {
        self.entries.retain(|entry| match entry {
            HistoryEntry::Document(snapshot) => snapshot.list != *list,
            HistoryEntry::Entries(_) => true,
        });
    }
}

/// The undo and redo stacks of all lists, each bounded to [`CAPACITY`] entries.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct History {
    undo: BoundedStack,
    redo: BoundedStack,
}

impl History {
    /// Remembers the state before a new change; the redo stack no longer applies to it.
    pub(super) fn record(&mut self, entry: HistoryEntry) {
        self.undo.push(entry);
        self.redo.entries.clear();
    }

    /// Takes the entry the next step in `direction` restores.
    pub(super) fn take(&mut self, direction: Direction) -> Option<HistoryEntry> {
        self.stack_mut(direction).pop()
    }

    /// Stores `entry`, the state a step in `direction` left, so that the opposite step can
    /// restore it.
    pub(super) fn put_back(&mut self, direction: Direction, entry: HistoryEntry) {
        self.stack_mut(direction.opposite()).push(entry);
    }

    /// Returns `entry`, the part of a step in `direction` that could not be performed, to its
    /// stack, so that the next step in `direction` tries it again.
    pub(super) fn keep(&mut self, direction: Direction, entry: HistoryEntry) {
        self.stack_mut(direction).push(entry);
    }

    /// Follows a renamed or removed folder or list: snapshots move to the new path of their
    /// list; see [`BoundedStack::apply_entry_change`].
    pub(super) fn apply_entry_change(&mut self, change: EntryChange<'_>) {
        self.undo.apply_entry_change(change);
        self.redo.apply_entry_change(change);
    }

    /// Drops every snapshot of `list`, whose document no longer matches them, for example
    /// because another program changed its file.
    pub(super) fn forget(&mut self, list: &ListPath) {
        self.undo.forget(list);
        self.redo.forget(list);
    }

    fn stack_mut(&mut self, direction: Direction) -> &mut BoundedStack {
        match direction {
            Direction::Undo => &mut self.undo,
            Direction::Redo => &mut self.redo,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::Path;

    use rstest::rstest;
    use strum::IntoEnumIterator;
    use tempfile::TempDir;

    use super::*;
    use crate::config::IndentStyle;
    use crate::storage::bin::BinPath;
    use crate::storage::workspace::{Entry, Workspace};

    /// Returns the path of a list at `relative`, which only a scanned workspace can create.
    fn list_path(relative: &str) -> ListPath {
        let directory = TempDir::new().unwrap();
        fs::write(directory.path().join("list.md"), "").unwrap();
        let workspace =
            Workspace::scan(directory.path(), BinPath::default(), IndentStyle::default()).unwrap();
        let [Entry::List(list)] = workspace.entries() else {
            panic!("expected one list");
        };
        list.path()
            .moved(list.path().as_path(), Path::new(relative))
            .unwrap()
    }

    fn entry(list: &str, text: &str) -> HistoryEntry {
        HistoryEntry::Document(DocumentSnapshot {
            list: list_path(list),
            document: TodoDocument::parse(text),
            selection: None,
        })
    }

    fn trash(path: &str) -> HistoryEntry {
        HistoryEntry::Entries(EntryOperation::Trash(vec![PathBuf::from(path)]))
    }

    fn history_with(entries: &[HistoryEntry]) -> History {
        let mut history = History::default();
        for entry in entries {
            history.record(entry.clone());
        }
        history
    }

    #[test]
    fn undo_takes_the_newest_entry_first() {
        let first = entry("a.md", "- [ ] first\n");
        let second = entry("a.md", "- [ ] second\n");
        let mut history = history_with(&[first.clone(), second.clone()]);

        let system_under_test = [
            history.take(Direction::Undo),
            history.take(Direction::Undo),
            history.take(Direction::Undo),
        ];

        assert_eq!(system_under_test, [Some(second), Some(first), None]);
    }

    #[rstest]
    #[case::undo(Direction::Undo, Direction::Redo)]
    #[case::redo(Direction::Redo, Direction::Undo)]
    fn put_back_stores_on_the_opposite_stack(
        #[case] direction: Direction,
        #[case] opposite: Direction,
    ) {
        let stored = entry("a.md", "- [ ] a\n");
        let mut history = History::default();
        history.put_back(direction, stored.clone());

        let system_under_test = [history.take(direction), history.take(opposite)];

        assert_eq!(system_under_test, [None, Some(stored)]);
    }

    #[test]
    fn record_clears_the_redo_stack() {
        let undone = entry("a.md", "- [ ] undone\n");
        let new = entry("a.md", "- [ ] new\n");
        let mut history = History::default();
        history.put_back(Direction::Undo, undone);
        history.record(new);

        let system_under_test = history.take(Direction::Redo);

        assert_eq!(system_under_test, None);
    }

    #[test]
    fn stacks_forget_the_oldest_entry_beyond_their_capacity() {
        let entries: Vec<HistoryEntry> = (0..=CAPACITY)
            .map(|index| entry("a.md", &format!("- [ ] {index}\n")))
            .collect();
        let expected = &entries[1..];
        let mut history = history_with(&entries);

        let system_under_test: Vec<HistoryEntry> =
            std::iter::from_fn(|| history.take(Direction::Undo)).collect();

        assert_eq!(system_under_test.len(), CAPACITY);
        assert!(system_under_test.iter().rev().eq(expected));
    }

    #[test]
    fn forget_drops_only_the_entries_of_the_list() {
        let kept = entry("b.md", "- [ ] b\n");
        let mut history = history_with(&[entry("a.md", "- [ ] a\n"), kept.clone()]);
        history.put_back(Direction::Undo, entry("a.md", "- [ ] redo\n"));
        history.forget(&list_path("a.md"));

        let system_under_test = history;

        assert_eq!(system_under_test, history_with(&[kept]));
    }

    #[test]
    fn renamed_lists_keep_their_entries_under_the_new_path() {
        let mut history = history_with(&[entry("home/a.md", "- [ ] a\n")]);
        history.put_back(Direction::Undo, entry("home/a.md", "- [ ] redo\n"));
        let mut expected = history_with(&[entry("work/a.md", "- [ ] a\n")]);
        expected.put_back(Direction::Undo, entry("work/a.md", "- [ ] redo\n"));
        history.apply_entry_change(EntryChange::Renamed {
            from: Path::new("home"),
            to: Path::new("work"),
        });

        let system_under_test = history;

        assert_eq!(system_under_test, expected);
    }

    #[test]
    fn removed_lists_keep_their_entries_for_a_restore() {
        let mut history = history_with(&[entry("a.md", "- [ ] a\n"), entry("b.md", "- [ ] b\n")]);
        let expected = history.clone();
        history.apply_entry_change(EntryChange::Removed(Path::new("a.md")));

        let system_under_test = history;

        assert_eq!(system_under_test, expected);
    }

    #[test]
    fn entry_operations_keep_their_paths_when_entries_are_renamed() {
        let mut history = history_with(&[trash("home/a.md")]);
        let expected = history.clone();
        history.apply_entry_change(EntryChange::Renamed {
            from: Path::new("home"),
            to: Path::new("work"),
        });

        let system_under_test = history;

        assert_eq!(system_under_test, expected);
    }

    #[test]
    fn forget_keeps_entry_operations() {
        let mut history = history_with(&[trash("a.md"), entry("a.md", "- [ ] a\n")]);
        history.forget(&list_path("a.md"));

        let system_under_test = history;

        assert_eq!(system_under_test, history_with(&[trash("a.md")]));
    }

    #[rstest]
    #[case::undo(Direction::Undo)]
    #[case::redo(Direction::Redo)]
    fn keep_returns_the_entry_to_its_own_stack(#[case] direction: Direction) {
        let kept = trash("a.md");
        let mut history = History::default();
        history.keep(direction, kept.clone());

        let system_under_test = [history.take(direction.opposite()), history.take(direction)];

        assert_eq!(system_under_test, [None, Some(kept)]);
    }

    #[test]
    fn workspace_paths_name_the_entries_before_the_operation() {
        let operation = EntryOperation::Relocate(vec![
            Relocation::new(PathBuf::from("a.md"), PathBuf::from("b.md")),
            Relocation::new(PathBuf::from("c"), PathBuf::from("d")),
        ]);
        let expected = [Path::new("a.md"), Path::new("c")];

        let system_under_test = operation.workspace_paths();

        assert_eq!(system_under_test, expected);
    }

    #[test]
    fn reversed_relocation_moves_back() {
        let relocation = Relocation::new(PathBuf::from("a.md"), PathBuf::from("b.md"));
        let expected = Relocation::new(PathBuf::from("b.md"), PathBuf::from("a.md"));

        let system_under_test = relocation.reversed();

        assert_eq!(system_under_test, expected);
    }

    #[test]
    fn directions_are_named_like_their_actions() {
        let expected = ["undo", "redo"];

        let system_under_test: Vec<String> = Direction::iter().map(|d| d.to_string()).collect();

        assert_eq!(system_under_test, expected);
    }
}

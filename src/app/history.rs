//! Undo and redo: [`History`] remembers the documents that edits replaced.
//!
//! An entry holds the whole document on the other side of a change, with the task that was
//! selected there. Undoing swaps it with the document in the workspace and moves the entry to
//! the redo stack, which then holds the state the undo left; redoing is the same swap in the
//! other direction. Keeping one snapshot per change instead of both sides halves the memory
//! and makes undo and redo one operation.

use std::collections::VecDeque;
use std::fmt::{self, Display, Formatter};

use crate::app::lists::EntryChange;
use crate::markdown::document::{TaskId, TodoDocument};
use crate::storage::workspace::ListPath;

/// How many changes each stack remembers; older ones are forgotten.
const CAPACITY: usize = 100;

/// Which way through the history a step goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, strum::EnumIter)]
pub(super) enum Direction {
    /// Back to the state before the last change.
    Undo,
    /// Forward again to the state the last undo left.
    Redo,
}

impl Display for Direction {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Undo => "undo",
            Self::Redo => "redo",
        })
    }
}

/// The state of a list on the other side of a change: its document and selected task.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct HistoryEntry {
    pub(super) list: ListPath,
    pub(super) document: TodoDocument,
    pub(super) selection: Option<TaskId>,
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

    /// Keeps the entries whose list still exists after `change`, under its new path.
    fn apply_entry_change(&mut self, change: EntryChange<'_>) {
        self.entries = std::mem::take(&mut self.entries)
            .into_iter()
            .filter_map(|mut entry| {
                entry.list = change.list_path(entry.list)?;
                Some(entry)
            })
            .collect();
    }

    fn forget(&mut self, list: &ListPath) {
        self.entries.retain(|entry| entry.list != *list);
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
        let opposite = match direction {
            Direction::Undo => Direction::Redo,
            Direction::Redo => Direction::Undo,
        };
        self.stack_mut(opposite).push(entry);
    }

    /// Follows a renamed or removed folder or list: entries move to the new path of their
    /// list or are dropped with it.
    pub(super) fn apply_entry_change(&mut self, change: EntryChange<'_>) {
        self.undo.apply_entry_change(change);
        self.redo.apply_entry_change(change);
    }

    /// Drops every entry of `list`, whose document no longer matches them, for example
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
    use crate::storage::workspace::{Entry, Workspace};

    /// Returns the path of a list at `relative`, which only a scanned workspace can create.
    fn list_path(relative: &str) -> ListPath {
        let directory = TempDir::new().unwrap();
        fs::write(directory.path().join("list.md"), "").unwrap();
        let workspace = Workspace::scan(directory.path(), IndentStyle::default()).unwrap();
        let [Entry::List(list)] = workspace.entries() else {
            panic!("expected one list");
        };
        list.path()
            .moved(list.path().as_path(), Path::new(relative))
            .unwrap()
    }

    fn entry(list: &str, text: &str) -> HistoryEntry {
        HistoryEntry {
            list: list_path(list),
            document: TodoDocument::parse(text),
            selection: None,
        }
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
    fn removed_lists_lose_their_entries() {
        let kept = entry("b.md", "- [ ] b\n");
        let mut history = history_with(&[entry("a.md", "- [ ] a\n"), kept.clone()]);
        history.apply_entry_change(EntryChange::Removed(Path::new("a.md")));

        let system_under_test = history;

        assert_eq!(system_under_test, history_with(&[kept]));
    }

    #[test]
    fn directions_are_named_like_their_actions() {
        let expected = ["undo", "redo"];

        let system_under_test: Vec<String> = Direction::iter().map(|d| d.to_string()).collect();

        assert_eq!(system_under_test, expected);
    }
}

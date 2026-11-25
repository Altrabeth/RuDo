//! Ordering the lists pane by what the lists contain ([`ListOrder`]).
//!
//! Sorting the lists pane is a view: it reorders the rows and never writes a file. The tasks of
//! every list are summed up whenever the tree is built, like the `done/total` counters, so the
//! order follows every edit and reload without a cache; a folder sums up every list below it
//! that the pane shows.
//! The workspace keeps the entries of every folder in display order, folders first and by name,
//! and the sort is stable, so ties keep the order by name.

use std::cmp::Ordering;

use jiff::civil::Date;

use crate::app::entry_visibility::HiddenEntries;
use crate::app::state::{App, Focus};
use crate::config::ListOrder;
use crate::domain::priority;
use crate::markdown::document::TodoDocument;
use crate::markdown::task_line::TaskLine;
use crate::storage::workspace::Entry;

impl App {
    /// Orders the lists pane by `order` for the rest of the session, keeping the selection;
    /// nothing is written.
    pub(super) fn sort_lists(&mut self, order: ListOrder) {
        let viewport_height = self.viewport_height(Focus::Lists);
        self.lists
            .set_order(&self.workspace, order, viewport_height);
    }

    /// Keeps the selection of the lists pane in view after the tasks of a list changed, which
    /// can move the list and its folders in an order by contents.
    pub(super) fn list_contents_changed(&mut self) {
        match self.lists.order() {
            ListOrder::Name => {}
            ListOrder::Due | ListOrder::Priority | ListOrder::OpenTasks | ListOrder::TotalTasks => {
                let viewport_height = self.viewport_height(Focus::Lists);
                self.lists.refresh(&self.workspace, viewport_height);
            }
        }
    }
}

/// A folder or list of the workspace, a folder with its entries in the order of the lists pane.
#[derive(Debug)]
pub(super) struct OrderedEntry<'workspace> {
    entry: &'workspace Entry,
    /// What the list or every readable list below the folder contains; `None` for a list that
    /// cannot be read.
    contents: Option<Contents>,
    entries: Vec<OrderedEntry<'workspace>>,
}

impl<'workspace> OrderedEntry<'workspace> {
    /// Sums up `entry` and orders the entries of a folder that `hidden_entries` shows by
    /// `order`.
    fn new(entry: &'workspace Entry, order: ListOrder, hidden_entries: HiddenEntries) -> Self {
        match entry {
            Entry::List(list) => Self {
                entry,
                contents: list
                    .content()
                    .ok()
                    .map(|loaded| Contents::of_document(loaded.document())),
                entries: Vec::new(),
            },
            Entry::Folder(folder) => {
                let entries = ordered_entries(folder.entries(), order, hidden_entries);
                let contents = entries
                    .iter()
                    .filter_map(|ordered| ordered.contents)
                    .fold(Contents::default(), Contents::add);
                Self {
                    entry,
                    contents: Some(contents),
                    entries,
                }
            }
        }
    }

    /// Returns the folder or list.
    pub(super) fn entry(&self) -> &'workspace Entry {
        self.entry
    }

    /// Returns the entries of a folder in order; none for a list.
    pub(super) fn entries(&self) -> &[Self] {
        &self.entries
    }
}

/// Returns those of `entries`, which are in display order, that `hidden_entries` shows,
/// ordered by `order`, which is not [`ListOrder::Name`], with the entries of every folder
/// ordered the same way.
pub(super) fn ordered_entries(
    entries: &[Entry],
    order: ListOrder,
    hidden_entries: HiddenEntries,
) -> Vec<OrderedEntry<'_>> {
    let mut ordered: Vec<OrderedEntry<'_>> = entries
        .iter()
        .filter(|entry| hidden_entries.shows(entry))
        .map(|entry| OrderedEntry::new(entry, order, hidden_entries))
        .collect();
    // A stable sort, so that ties keep the order by name.
    ordered.sort_by(|first, second| compare(order, first, second));
    ordered
}

/// Orders folders before lists, then by `order`, and lists that cannot be read last.
fn compare(order: ListOrder, first: &OrderedEntry<'_>, second: &OrderedEntry<'_>) -> Ordering {
    first.entry.kind().cmp(&second.entry.kind()).then_with(|| {
        match (first.contents, second.contents) {
            (Some(first), Some(second)) => first.compare(second, order),
            (first, second) => first.is_none().cmp(&second.is_none()),
        }
    })
}

/// What the tasks of a list, or of all lists below a folder, add up to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
struct Contents {
    /// Tasks that are neither done nor cancelled.
    open: usize,
    total: usize,
    /// The earliest due date of the open tasks.
    earliest_due: Option<Date>,
    /// The highest [`priority::level`] of the open tasks; `None` without open tasks.
    highest_priority: Option<usize>,
}

impl Contents {
    /// Sums up the tasks of `document`, subtasks included.
    fn of_document(document: &TodoDocument) -> Self {
        document
            .tasks()
            .fold(Self::default(), |contents, node| contents.with(node.task()))
    }

    /// Returns these contents with `task` added.
    fn with(self, task: &TaskLine) -> Self {
        let total = self.total + 1;
        if task.status().is_completed() {
            return Self { total, ..self };
        }
        Self {
            open: self.open + 1,
            total,
            earliest_due: earliest(self.earliest_due, task.due()),
            highest_priority: self
                .highest_priority
                .max(Some(priority::level(task.priority()))),
        }
    }

    /// Returns the contents of two lists or folders together.
    fn add(self, other: Self) -> Self {
        Self {
            open: self.open + other.open,
            total: self.total + other.total,
            earliest_due: earliest(self.earliest_due, other.earliest_due),
            highest_priority: self.highest_priority.max(other.highest_priority),
        }
    }

    /// Orders these contents before `other` if they come first in `order`.
    fn compare(self, other: Self, order: ListOrder) -> Ordering {
        match order {
            ListOrder::Name => Ordering::Equal,
            ListOrder::Due => self.urgency().cmp(&other.urgency()),
            ListOrder::Priority => other
                .highest_priority
                .cmp(&self.highest_priority)
                .then_with(|| self.urgency().cmp(&other.urgency())),
            ListOrder::OpenTasks => other.open.cmp(&self.open),
            ListOrder::TotalTasks => other.total.cmp(&self.total),
        }
    }

    /// The most urgent first: the earliest due date, contents without one last.
    fn urgency(self) -> (bool, Option<Date>) {
        (self.earliest_due.is_none(), self.earliest_due)
    }
}

/// Returns the earlier of two dates, or the one that is given.
fn earliest(first: Option<Date>, second: Option<Date>) -> Option<Date> {
    first.into_iter().chain(second).min()
}

#[cfg(test)]
mod tests {
    use std::fs;

    use jiff::civil::date;
    use rstest::rstest;
    use strum::IntoEnumIterator;
    use tempfile::TempDir;

    use super::*;
    use crate::config::IndentStyle;
    use crate::storage::bin::BinPath;
    use crate::storage::workspace::Workspace;

    fn names(entries: &[OrderedEntry<'_>]) -> Vec<String> {
        entries
            .iter()
            .flat_map(|ordered| {
                let name = match ordered.entry() {
                    Entry::Folder(folder) => format!("{}/", folder.name()),
                    Entry::List(list) => list.name().to_owned(),
                };
                std::iter::once(name).chain(names(ordered.entries()))
            })
            .collect()
    }

    #[rstest]
    #[case::empty("", Contents::default())]
    #[case::open_and_completed(
        "- [ ] a [due:: 2026-03-20] [priority:: low]\n\t- [/] b [due:: 2026-03-18]\n\
         - [x] c [due:: 2026-01-01] [priority:: highest]\n- [-] d\n",
        Contents {
            open: 2,
            total: 4,
            earliest_due: Some(date(2026, 3, 18)),
            highest_priority: Some(priority::level(None)),
        }
    )]
    #[case::only_completed(
        "- [x] a [due:: 2026-01-01]\n",
        Contents { open: 0, total: 1, earliest_due: None, highest_priority: None }
    )]
    fn contents_sum_up_open_and_all_tasks(#[case] text: &str, #[case] expected: Contents) {
        let document = TodoDocument::parse(text);

        let system_under_test = Contents::of_document(&document);

        assert_eq!(system_under_test, expected);
    }

    #[test]
    fn contents_add_up_counts_and_keep_the_most_urgent_and_highest() {
        let first = Contents {
            open: 1,
            total: 2,
            earliest_due: Some(date(2026, 3, 20)),
            highest_priority: None,
        };
        let second = Contents {
            open: 3,
            total: 3,
            earliest_due: Some(date(2026, 3, 10)),
            highest_priority: Some(4),
        };
        let expected = Contents {
            open: 4,
            total: 5,
            earliest_due: Some(date(2026, 3, 10)),
            highest_priority: Some(4),
        };

        let system_under_test = first.add(second);

        assert_eq!(system_under_test, expected);
    }

    #[test]
    fn every_order_but_name_puts_lists_that_cannot_be_read_last() {
        let root = TempDir::new().unwrap();
        fs::write(root.path().join("a.md"), [0xff]).unwrap();
        fs::write(root.path().join("b.md"), "").unwrap();
        fs::write(root.path().join("c.md"), "").unwrap();
        let workspace =
            Workspace::scan(root.path(), BinPath::default(), IndentStyle::default()).unwrap();
        let expected = ["b", "c", "a"];

        for order in ListOrder::iter().filter(|order| *order != ListOrder::Name) {
            let system_under_test =
                ordered_entries(workspace.entries(), order, HiddenEntries::Hidden);

            assert_eq!(names(&system_under_test), expected, "{order:?}");
        }
    }

    #[test]
    fn folders_stay_first_and_are_ordered_by_everything_below_them() {
        let root = TempDir::new().unwrap();
        fs::create_dir_all(root.path().join("few/deep")).unwrap();
        fs::create_dir_all(root.path().join("many")).unwrap();
        fs::write(root.path().join("few/deep/x.md"), "- [ ] a\n").unwrap();
        fs::write(root.path().join("many/y.md"), "- [ ] a\n- [ ] b\n").unwrap();
        fs::write(root.path().join("many/z.md"), "- [ ] a\n").unwrap();
        fs::write(
            root.path().join("big.md"),
            "- [ ] a\n- [ ] b\n- [ ] c\n- [ ] d\n",
        )
        .unwrap();
        let workspace =
            Workspace::scan(root.path(), BinPath::default(), IndentStyle::default()).unwrap();
        let expected = ["many/", "y", "z", "few/", "deep/", "x", "big"];

        let system_under_test = ordered_entries(
            workspace.entries(),
            ListOrder::TotalTasks,
            HiddenEntries::Hidden,
        );

        assert_eq!(names(&system_under_test), expected);
    }
}

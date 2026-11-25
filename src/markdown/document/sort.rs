//! Sorting a task together with its siblings.
//!
//! Sorting moves whole blocks, so subtasks and continuation lines stay with their task, and
//! every task keeps its id. Only the run of sibling blocks around the task is sorted: any other
//! line between two siblings, such as a heading, is a boundary that stays in place. Blank lines
//! between the blocks of a run stay in place as well.

use std::cmp::Reverse;
use std::ops::Range;

use jiff::civil::Date;

use crate::domain::priority;
use crate::markdown::task_line::TaskLine;

use super::{DocumentLine, EditOutcome, TaskId, TodoDocument};

/// What [`TodoDocument::sort_siblings`] orders tasks by.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, strum::EnumIter)]
pub enum SortKey {
    /// Highest priority first. A task without a priority field has normal priority, between
    /// medium and low.
    Priority,
    /// Earliest due date first, tasks without a due date last.
    DueDate,
    /// By description, ignoring case.
    Alphabetical,
    /// Open and in progress tasks before done and cancelled ones.
    Status,
}

impl SortKey {
    /// Returns the indices of `tasks` in sorted order; equal tasks keep their order.
    fn order(self, tasks: &[&TaskLine]) -> Vec<usize> {
        match self {
            Self::Priority => stable_order(tasks, priority_key),
            Self::DueDate => stable_order(tasks, due_key),
            Self::Alphabetical => stable_order(tasks, alphabetical_key),
            Self::Status => stable_order(tasks, status_key),
        }
    }
}

impl TodoDocument {
    /// Sorts the block of a task and the blocks of its siblings by `key`.
    ///
    /// The sort is stable, so sorting twice changes nothing the second time. Only the
    /// siblings that no line other than a blank line separates from the task are sorted, and
    /// each block takes over the indentation of the place it moves to.
    ///
    /// Returns [`EditOutcome::Unchanged`] if the order stays the same or if `id` is not in
    /// this document.
    ///
    /// # Examples
    ///
    /// ```
    /// use rudo::markdown::document::{EditOutcome, SortKey, TodoDocument};
    ///
    /// let mut document = TodoDocument::parse("- [ ] b\n  - [ ] b1\n- [ ] A\n\n# Later\n- [ ] a\n");
    /// let b = document.tasks().next().expect("a task").id();
    ///
    /// assert_eq!(document.sort_siblings(b, SortKey::Alphabetical), EditOutcome::Changed);
    /// assert_eq!(document.to_string(), "- [ ] A\n- [ ] b\n  - [ ] b1\n\n# Later\n- [ ] a\n");
    /// assert_eq!(document.sort_siblings(b, SortKey::Alphabetical), EditOutcome::Unchanged);
    /// ```
    pub fn sort_siblings(&mut self, id: TaskId, key: SortKey) -> EditOutcome {
        let ranges = self.sibling_run(id);
        let tasks: Vec<&TaskLine> = ranges
            .iter()
            .filter_map(|range| self.lines[range.start].task().map(|(_, task)| task))
            .collect();
        let order = key.order(&tasks);
        if order.iter().enumerate().all(|(slot, &index)| slot == index) {
            return EditOutcome::Unchanged;
        }
        let indents: Vec<String> = tasks.iter().map(|task| task.indent().to_owned()).collect();
        for (slot, &index) in order.iter().enumerate() {
            if indents[index] != indents[slot] {
                self.reindent_lines(ranges[index].clone(), &indents[index], &indents[slot]);
            }
        }
        let destinations = destinations(&ranges, &order);
        let span = ranges[0].start..ranges[ranges.len() - 1].end;
        self.rearrange_lines(span, |lines| permute(lines, destinations));
        EditOutcome::Changed
    }

    /// Returns the line ranges of the blocks of the task `id` and of its siblings that only
    /// blank lines separate from it, in document order; none if `id` is not in this document.
    fn sibling_run(&self, id: TaskId) -> Vec<Range<usize>> {
        let Some(parent) = self.node(id).map(|node| node.parent) else {
            return Vec::new();
        };
        let siblings: Vec<TaskId> = self
            .tasks()
            .filter(|node| node.parent == parent)
            .map(|node| node.id)
            .collect();
        let Some(position) = siblings.iter().position(|&sibling| sibling == id) else {
            return Vec::new();
        };
        let mut ranges: Vec<Range<usize>> = siblings
            .into_iter()
            .filter_map(|sibling| self.block_range(sibling))
            .collect();
        let separated = |pair: &[Range<usize>]| {
            !self.lines[pair[0].end..pair[1].start]
                .iter()
                .all(DocumentLine::is_blank)
        };
        let end = ranges[position..]
            .windows(2)
            .position(separated)
            .map_or(ranges.len(), |boundary| position + boundary + 1);
        let start = ranges[..=position]
            .windows(2)
            .rposition(separated)
            .map_or(0, |boundary| boundary + 1);
        ranges.truncate(end);
        ranges.drain(..start);
        ranges
    }
}

/// Returns the indices of `tasks` sorted by `key`.
fn stable_order<K: Ord>(tasks: &[&TaskLine], key: impl Fn(&TaskLine) -> K) -> Vec<usize> {
    let mut order: Vec<usize> = (0..tasks.len()).collect();
    // A stable sort, so that equal tasks are not reordered on every run.
    order.sort_by_cached_key(|&index| key(tasks[index]));
    order
}

/// Highest priority first; an unknown value counts as normal priority, as when stepping it.
fn priority_key(task: &TaskLine) -> Reverse<usize> {
    Reverse(priority::level(task.priority()))
}

/// Earliest due date first; a task without a due date, or with a value that is no date, last.
fn due_key(task: &TaskLine) -> (bool, Option<Date>) {
    let due = task.due();
    (due.is_none(), due)
}

fn alphabetical_key(task: &TaskLine) -> String {
    task.description().to_lowercase()
}

/// Open tasks (`false`) before completed ones (`true`).
fn status_key(task: &TaskLine) -> bool {
    task.status().is_completed()
}

/// Returns for every line of the span from the first to the last of `ranges` its index in the
/// span after the blocks were put in `order`. The lines between two blocks keep their place.
fn destinations(ranges: &[Range<usize>], order: &[usize]) -> Vec<usize> {
    let span_start = ranges[0].start;
    let mut destinations = vec![0; ranges[ranges.len() - 1].end - span_start];
    let sources = order.iter().enumerate().flat_map(|(slot, &index)| {
        let gap_end = ranges
            .get(slot + 1)
            .map_or(ranges[slot].end, |next| next.start);
        ranges[index].clone().chain(ranges[slot].end..gap_end)
    });
    for (destination, source) in sources.enumerate() {
        destinations[source - span_start] = destination;
    }
    destinations
}

/// Moves every item to the index that `destinations` holds for it, in place, by following each
/// cycle of the permutation with swaps.
fn permute<T>(items: &mut [T], mut destinations: Vec<usize>) {
    for index in 0..items.len() {
        while destinations[index] != index {
            let destination = destinations[index];
            items.swap(index, destination);
            destinations.swap(index, destination);
        }
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;
    use strum::IntoEnumIterator;

    use super::*;

    const UNKNOWN: TaskId = TaskId(99);

    /// Returns the id of the first task with `description`.
    fn id_of(document: &TodoDocument, description: &str) -> TaskId {
        document
            .tasks()
            .find(|node| node.task().description() == description)
            .unwrap()
            .id()
    }

    /// Sorts the siblings of the task `reference` and returns the outcome and the text.
    fn sort(text: &str, reference: &str, key: SortKey) -> (EditOutcome, String) {
        let mut document = TodoDocument::parse(text);
        let id = id_of(&document, reference);
        let outcome = document.sort_siblings(id, key);
        (outcome, document.to_string())
    }

    #[rstest]
    #[case::priority_highest_first_and_normal_between_medium_and_low(
        concat!(
            "- [ ] normal\n- [ ] low [priority:: low]\n- [ ] high [priority:: high]\n",
            "- [ ] lowest [priority:: lowest]\n- [ ] medium [priority:: medium]\n",
            "- [ ] highest [priority:: highest]\n",
        ),
        SortKey::Priority,
        concat!(
            "- [ ] highest [priority:: highest]\n- [ ] high [priority:: high]\n",
            "- [ ] medium [priority:: medium]\n- [ ] normal\n- [ ] low [priority:: low]\n",
            "- [ ] lowest [priority:: lowest]\n",
        )
    )]
    #[case::priority_unknown_value_is_normal(
        "- [ ] low [priority:: low]\n- [ ] odd [priority:: urgent]\n",
        SortKey::Priority,
        "- [ ] odd [priority:: urgent]\n- [ ] low [priority:: low]\n"
    )]
    #[case::due_date_earliest_first_and_none_last(
        concat!(
            "- [ ] none\n- [ ] late [due:: 2026-12-24]\n- [ ] odd [due:: someday]\n",
            "- [ ] early [due:: 2026-11-30]\n",
        ),
        SortKey::DueDate,
        concat!(
            "- [ ] early [due:: 2026-11-30]\n- [ ] late [due:: 2026-12-24]\n- [ ] none\n",
            "- [ ] odd [due:: someday]\n",
        )
    )]
    #[case::alphabetical_ignores_case(
        "- [ ] banana\n- [ ] Apple\n- [ ] cherry\n",
        SortKey::Alphabetical,
        "- [ ] Apple\n- [ ] banana\n- [ ] cherry\n"
    )]
    #[case::status_open_before_completed(
        "- [x] done\n- [ ] todo\n- [-] cancelled\n- [/] doing\n- [?] other\n",
        SortKey::Status,
        "- [ ] todo\n- [/] doing\n- [?] other\n- [x] done\n- [-] cancelled\n"
    )]
    fn sorts_siblings_by_key(#[case] text: &str, #[case] key: SortKey, #[case] expected: &str) {
        let reference = TodoDocument::parse(text)
            .tasks()
            .next()
            .unwrap()
            .task()
            .description()
            .to_owned();

        let system_under_test = sort(text, &reference, key);

        assert_eq!(
            system_under_test,
            (EditOutcome::Changed, expected.to_owned())
        );
    }

    #[rstest]
    #[case::subtasks_and_continuations_move_with_their_parent(
        "- [ ] b\n  notes on b\n  - [ ] b1\n- [ ] a\n",
        "a",
        "- [ ] a\n- [ ] b\n  notes on b\n  - [ ] b1\n"
    )]
    #[case::heading_is_a_boundary(
        "- [ ] d\n- [ ] c\n# Later\n- [ ] b\n- [ ] a\n",
        "d",
        "- [ ] c\n- [ ] d\n# Later\n- [ ] b\n- [ ] a\n"
    )]
    #[case::run_after_a_boundary(
        "- [ ] d\n- [ ] c\nParagraph\n- [ ] b\n- [ ] a\n",
        "a",
        "- [ ] d\n- [ ] c\nParagraph\n- [ ] a\n- [ ] b\n"
    )]
    #[case::blank_lines_between_blocks_stay_in_place(
        "- [ ] c\n\n- [ ] b\n  - [ ] b1\n\n\n- [ ] a\n",
        "b",
        "- [ ] a\n\n- [ ] b\n  - [ ] b1\n\n\n- [ ] c\n"
    )]
    #[case::only_siblings_of_a_subtask(
        "- [ ] z\n  - [ ] y\n  - [ ] x\n- [ ] a\n",
        "y",
        "- [ ] z\n  - [ ] x\n  - [ ] y\n- [ ] a\n"
    )]
    #[case::blocks_take_the_indentation_of_their_place(
        "- [ ] p\n    - [ ] b\n  - [ ] a\n    - [ ] a1\n",
        "a",
        "- [ ] p\n    - [ ] a\n      - [ ] a1\n  - [ ] b\n"
    )]
    #[case::last_line_without_newline("- [ ] b\n- [ ] a", "a", "- [ ] a\n- [ ] b")]
    #[case::crlf("- [ ] b\r\n- [ ] a\r\n", "a", "- [ ] a\r\n- [ ] b\r\n")]
    #[case::stable_for_equal_keys(
        "- [ ] b\n- [ ] a\n- [ ] B\n",
        "a",
        "- [ ] a\n- [ ] b\n- [ ] B\n"
    )]
    fn sorts_only_the_run_of_sibling_blocks(
        #[case] text: &str,
        #[case] reference: &str,
        #[case] expected: &str,
    ) {
        let system_under_test = sort(text, reference, SortKey::Alphabetical);

        assert_eq!(
            system_under_test,
            (EditOutcome::Changed, expected.to_owned())
        );
    }

    #[rstest]
    #[case::already_sorted("- [ ] a\n- [ ] b\n", "a")]
    #[case::only_task("- [ ] b\n  - [ ] a\n", "b")]
    #[case::separated_by_a_boundary("- [ ] b\n# Heading\n- [ ] a\n", "b")]
    #[case::equal_keys_keep_their_order("- [ ] B\n- [ ] b\n", "b")]
    fn unchanged_order_is_unchanged(#[case] text: &str, #[case] reference: &str) {
        let system_under_test = sort(text, reference, SortKey::Alphabetical);

        assert_eq!(system_under_test, (EditOutcome::Unchanged, text.to_owned()));
    }

    #[test]
    fn unknown_task_is_unchanged_for_every_key() {
        let text = "- [ ] b\n- [ ] a [priority:: high]\n";
        let mut document = TodoDocument::parse(text);
        let expected: Vec<EditOutcome> = SortKey::iter().map(|_| EditOutcome::Unchanged).collect();

        let system_under_test: Vec<EditOutcome> = SortKey::iter()
            .map(|key| document.sort_siblings(UNKNOWN, key))
            .collect();

        assert_eq!(system_under_test, expected);
        assert_eq!(document.to_string(), text);
    }

    #[test]
    fn sorted_tasks_keep_their_ids() {
        let mut document = TodoDocument::parse("- [ ] b\n  - [ ] b1\n- [ ] a\n");
        let a = id_of(&document, "a");
        let b = id_of(&document, "b");
        let b1 = id_of(&document, "b1");
        let expected = vec![a, b, b1];

        let system_under_test = document.sort_siblings(b, SortKey::Alphabetical);

        assert_eq!(system_under_test, EditOutcome::Changed);
        assert_eq!(
            document.tasks().map(|node| node.id()).collect::<Vec<_>>(),
            expected
        );
    }

    #[rstest]
    #[case::identity(vec![0, 1, 2], vec!['a', 'b', 'c'])]
    #[case::rotation(vec![1, 2, 0], vec!['c', 'a', 'b'])]
    #[case::two_cycles(vec![1, 0, 3, 2], vec!['b', 'a', 'd', 'c'])]
    fn permute_moves_items_to_their_destinations(
        #[case] destinations: Vec<usize>,
        #[case] expected: Vec<char>,
    ) {
        let mut items: Vec<char> = ('a'..).take(destinations.len()).collect();

        let system_under_test = {
            permute(&mut items, destinations);
            items
        };

        assert_eq!(system_under_test, expected);
    }
}

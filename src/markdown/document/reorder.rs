//! Moving task blocks among their siblings, indenting and outdenting them.
//!
//! Every operation keeps the ids of the moved tasks and moves or shifts whole blocks, so
//! continuation lines always stay with their task. Lines between two sibling blocks, such as a
//! heading or a blank line, stay where they are.

use std::collections::HashSet;
use std::ops::Range;

use crate::config::IndentStyle;
use crate::markdown::task_line::indentation_width;

use super::insert::replace_indent;
use super::{DocumentLine, EditOutcome, LineKind, TaskId, TodoDocument};

impl TodoDocument {
    /// Swaps the block of a task with the block of its previous sibling.
    ///
    /// Siblings are tasks with the same parent. If their indentation differs, each block takes
    /// over the indentation of the place it moves to, so the task tree keeps its shape.
    ///
    /// Returns [`EditOutcome::Unchanged`] for a first sibling or if `id` is not in this
    /// document.
    ///
    /// # Examples
    ///
    /// ```
    /// use rudo::markdown::document::{EditOutcome, TodoDocument};
    ///
    /// let mut document = TodoDocument::parse("- [ ] a\n\n# Later\n- [ ] b\n  notes on b\n");
    /// let b = document.tasks().nth(1).expect("two tasks").id();
    ///
    /// assert_eq!(document.move_up(b), EditOutcome::Changed);
    /// assert_eq!(document.to_string(), "- [ ] b\n  notes on b\n\n# Later\n- [ ] a\n");
    /// assert_eq!(document.move_up(b), EditOutcome::Unchanged);
    /// ```
    pub fn move_up(&mut self, id: TaskId) -> EditOutcome {
        match self.previous_sibling(id) {
            Some(previous) => self.swap_blocks(previous, id),
            None => EditOutcome::Unchanged,
        }
    }

    /// Swaps the block of a task with the block of its next sibling, as
    /// [`TodoDocument::move_up`] does with the previous one.
    ///
    /// Returns [`EditOutcome::Unchanged`] for a last sibling or if `id` is not in this
    /// document.
    pub fn move_down(&mut self, id: TaskId) -> EditOutcome {
        match self.next_sibling(id) {
            Some(next) => self.swap_blocks(id, next),
            None => EditOutcome::Unchanged,
        }
    }

    /// Makes a task with its block the last subtask of its previous sibling.
    ///
    /// The block takes the indentation of the previous sibling's last subtask, or one level of
    /// [`TodoDocument::indent_style`] (with `fallback_indent`) beyond the previous sibling if it
    /// has none. Lines that are not tasks between the two blocks stay where they are.
    ///
    /// Returns [`EditOutcome::Unchanged`] for a first sibling or if `id` is not in this
    /// document.
    ///
    /// # Examples
    ///
    /// ```
    /// use rudo::config::IndentStyle;
    /// use rudo::markdown::document::{EditOutcome, TodoDocument};
    ///
    /// let mut document = TodoDocument::parse("- [ ] a\n- [ ] b\n  notes on b\n");
    /// let b = document.tasks().nth(1).expect("two tasks").id();
    ///
    /// assert_eq!(document.indent(b, IndentStyle::Tab), EditOutcome::Changed);
    /// assert_eq!(document.to_string(), "- [ ] a\n\t- [ ] b\n\t  notes on b\n");
    /// assert_eq!(document.parent(b), document.tasks().next().map(|node| node.id()));
    /// ```
    pub fn indent(&mut self, id: TaskId, fallback_indent: IndentStyle) -> EditOutcome {
        let Some(previous) = self.previous_sibling(id) else {
            return EditOutcome::Unchanged;
        };
        let Some(new_indent) = self.child_indent(previous, fallback_indent) else {
            return EditOutcome::Unchanged;
        };
        self.reindent_block(id, &new_indent)
    }

    /// Moves a task with its block one level up by giving it the indentation of its parent.
    ///
    /// As in Markdown editors such as Obsidian (`shift-tab`) and in vim (`<<`), only the
    /// indentation of the block changes and no line moves. Later siblings of the task are
    /// therefore indented deeper than it afterwards and become its subtasks.
    ///
    /// Returns [`EditOutcome::Unchanged`] for a top-level task or if `id` is not in this
    /// document.
    ///
    /// # Examples
    ///
    /// ```
    /// use rudo::markdown::document::{EditOutcome, TodoDocument};
    ///
    /// let mut document = TodoDocument::parse("- [ ] a\n  - [ ] b\n  - [ ] c\n");
    /// let b = document.tasks().nth(1).expect("three tasks").id();
    /// let c = document.tasks().nth(2).expect("three tasks").id();
    ///
    /// assert_eq!(document.outdent(b), EditOutcome::Changed);
    /// assert_eq!(document.to_string(), "- [ ] a\n- [ ] b\n  - [ ] c\n");
    /// assert_eq!(document.parent(c), Some(b));
    /// ```
    pub fn outdent(&mut self, id: TaskId) -> EditOutcome {
        let Some(new_indent) = self
            .parent(id)
            .and_then(|parent| self.task_line(parent))
            .map(|parent| parent.indent().to_owned())
        else {
            return EditOutcome::Unchanged;
        };
        self.reindent_block(id, &new_indent)
    }

    /// Indents the tasks `ids` with [`TodoDocument::indent`], keeping their relation to each
    /// other; a task inside the block of another task of `ids` moves with that block.
    ///
    /// The tasks are indented in document order, so consecutive siblings all become subtasks
    /// of the sibling before the first of them. If the first has no previous sibling, the whole
    /// run stays where it is instead of becoming subtasks of its first task.
    ///
    /// Returns [`EditOutcome::Changed`] if any task moved.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::collections::HashSet;
    ///
    /// use rudo::config::IndentStyle;
    /// use rudo::markdown::document::{EditOutcome, TodoDocument};
    ///
    /// let mut document = TodoDocument::parse("- [ ] a\n- [ ] b\n- [ ] c\n");
    /// let selected: HashSet<_> = document.tasks().skip(1).map(|node| node.id()).collect();
    ///
    /// assert_eq!(document.indent_blocks(&selected, IndentStyle::Tab), EditOutcome::Changed);
    /// assert_eq!(document.to_string(), "- [ ] a\n\t- [ ] b\n\t- [ ] c\n");
    /// ```
    pub fn indent_blocks(
        &mut self,
        ids: &HashSet<TaskId>,
        fallback_indent: IndentStyle,
    ) -> EditOutcome {
        let mut staying = HashSet::new();
        let mut outcome = EditOutcome::Unchanged;
        for (id, _) in self.outermost_blocks(ids) {
            let follows_a_staying_task = self
                .previous_sibling(id)
                .is_some_and(|previous| staying.contains(&previous));
            if follows_a_staying_task || self.indent(id, fallback_indent) == EditOutcome::Unchanged
            {
                staying.insert(id);
            } else {
                outcome = EditOutcome::Changed;
            }
        }
        outcome
    }

    /// Outdents the tasks `ids` with [`TodoDocument::outdent`]; a task inside the block of
    /// another task of `ids` moves with that block, and top-level tasks stay where they are.
    ///
    /// Returns [`EditOutcome::Changed`] if any task moved.
    pub fn outdent_blocks(&mut self, ids: &HashSet<TaskId>) -> EditOutcome {
        let mut outcome = EditOutcome::Unchanged;
        for (id, _) in self.outermost_blocks(ids) {
            if self.outdent(id) == EditOutcome::Changed {
                outcome = EditOutcome::Changed;
            }
        }
        outcome
    }

    fn previous_sibling(&self, id: TaskId) -> Option<TaskId> {
        let parent = self.node(id)?.parent;
        self.tasks()
            .take_while(|node| node.id != id)
            .filter(|node| node.parent == parent)
            .last()
            .map(|node| node.id)
    }

    fn next_sibling(&self, id: TaskId) -> Option<TaskId> {
        let parent = self.node(id)?.parent;
        self.tasks()
            .skip_while(|node| node.id != id)
            .skip(1)
            .find(|node| node.parent == parent)
            .map(|node| node.id)
    }

    /// Swaps the blocks of two siblings, `first` before `second`, keeping the lines between
    /// them in place. Each block takes the indentation of the other.
    fn swap_blocks(&mut self, first: TaskId, second: TaskId) -> EditOutcome {
        let (Some(first_range), Some(second_range), Some(first_task), Some(second_task)) = (
            self.block_range(first),
            self.block_range(second),
            self.task_line(first),
            self.task_line(second),
        ) else {
            return EditOutcome::Unchanged;
        };
        if first_task.indent() != second_task.indent() {
            let first_indent = first_task.indent().to_owned();
            let second_indent = second_task.indent().to_owned();
            self.reindent_lines(first_range.clone(), &first_indent, &second_indent);
            self.reindent_lines(second_range.clone(), &second_indent, &first_indent);
        }
        self.swap_ranges(&first_range, &second_range);
        EditOutcome::Changed
    }

    /// Rearranges `[first][gap][second]` into `[second][gap][first]`, in place.
    fn swap_ranges(&mut self, first: &Range<usize>, second: &Range<usize>) {
        self.rearrange_lines(first.start..second.end, |span| {
            span.rotate_left(second.start - first.start);
            span[second.len()..].rotate_left(first.len());
        });
    }

    /// Lets `rearrange` reorder the non-empty `span` of lines in place. A missing final
    /// newline stays at the end of the document.
    pub(super) fn rearrange_lines(
        &mut self,
        span: Range<usize>,
        rearrange: impl FnOnce(&mut [DocumentLine]),
    ) {
        let last = span.end - 1;
        let ends_unterminated = self.lines[last].ending.is_none();
        if ends_unterminated {
            self.lines[last].ending = Some(self.line_ending);
        }
        rearrange(&mut self.lines[span]);
        if ends_unterminated {
            self.lines[last].ending = None;
        }
    }

    /// Replaces the indentation of the task `id` with `new_indent` on every line of its block.
    fn reindent_block(&mut self, id: TaskId, new_indent: &str) -> EditOutcome {
        let (Some(range), Some(task)) = (self.block_range(id), self.task_line(id)) else {
            return EditOutcome::Unchanged;
        };
        if task.indent() == new_indent {
            return EditOutcome::Unchanged;
        }
        let old_indent = task.indent().to_owned();
        self.reindent_lines(range, &old_indent, new_indent);
        EditOutcome::Changed
    }

    /// Replaces the prefix `old_indent` with `new_indent` on every line in `range` that is not
    /// blank, which keeps the indentation of the lines relative to each other even where tabs
    /// and spaces are mixed.
    pub(super) fn reindent_lines(
        &mut self,
        range: Range<usize>,
        old_indent: &str,
        new_indent: &str,
    ) {
        let old_width = indentation_width(old_indent);
        for line in &mut self.lines[range] {
            match &mut line.kind {
                LineKind::Task(_, task) => {
                    let mut indent = task.indent().to_owned();
                    replace_indent(&mut indent, old_indent, old_width, new_indent);
                    task.set_indent(indent);
                }
                LineKind::Raw(text) if text.trim().is_empty() => {}
                LineKind::Raw(text) => replace_indent(text, old_indent, old_width, new_indent),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;
    use crate::config::IndentWidth;

    /// A document with nested tasks, a continuation line and blank lines.
    const TREE: &str = concat!(
        "# List\n",
        "- [ ] a\n",
        "  - [ ] b\n",
        "    continuation of b\n",
        "\n",
        "    - [ ] c\n",
        "\n",
        "  - [ ] d\n",
        "\n",
        "- [ ] e\n",
        "Paragraph\n",
        "- [ ] f",
    );

    const UNKNOWN: TaskId = TaskId(99);

    type Operation = fn(&mut TodoDocument, TaskId) -> EditOutcome;

    fn spaces(width: u8) -> IndentStyle {
        IndentStyle::Spaces(IndentWidth::new(width).unwrap())
    }

    fn indent_with_three_spaces(document: &mut TodoDocument, id: TaskId) -> EditOutcome {
        document.indent(id, spaces(3))
    }

    /// Returns the id of the first task with `description`.
    fn id_of(document: &TodoDocument, description: &str) -> TaskId {
        document
            .tasks()
            .find(|node| node.task().description() == description)
            .unwrap()
            .id()
    }

    /// Applies `operation` to the task `reference` and returns the outcome and the text.
    fn apply(text: &str, operation: Operation, reference: &str) -> (EditOutcome, String) {
        let mut document = TodoDocument::parse(text);
        let id = id_of(&document, reference);
        let outcome = operation(&mut document, id);
        (outcome, document.to_string())
    }

    /// Returns every task as its description and the description of its parent.
    fn outline(document: &TodoDocument) -> Vec<(String, Option<String>)> {
        let description = |id: TaskId| {
            document
                .tasks()
                .find(|node| node.id() == id)
                .map(|node| node.task().description().to_owned())
        };
        document
            .tasks()
            .map(|node| {
                (
                    node.task().description().to_owned(),
                    node.parent().and_then(description),
                )
            })
            .collect()
    }

    #[rstest]
    #[case::middle_sibling("- [ ] a\n- [ ] b\n- [ ] c\n", "b", "- [ ] b\n- [ ] a\n- [ ] c\n")]
    #[case::last_sibling_with_raw_lines_between(
        "- [ ] a\n  - [ ] a1\n\n# Heading\n- [ ] b\n  text of b\n",
        "b",
        "- [ ] b\n  text of b\n\n# Heading\n- [ ] a\n  - [ ] a1\n"
    )]
    #[case::nested_blocks_of_different_length(
        TREE,
        "d",
        concat!(
            "# List\n- [ ] a\n  - [ ] d\n\n  - [ ] b\n    continuation of b\n\n    - [ ] c\n",
            "\n- [ ] e\nParagraph\n- [ ] f",
        )
    )]
    #[case::over_paragraph_to_last_line(
        TREE,
        "f",
        concat!(
            "# List\n- [ ] a\n  - [ ] b\n    continuation of b\n\n    - [ ] c\n\n  - [ ] d\n",
            "\n- [ ] f\nParagraph\n- [ ] e",
        )
    )]
    #[case::mixed_indentation(
        "- [ ] a\n    - [ ] b\n  - [ ] c\n    - [ ] d\n",
        "c",
        "- [ ] a\n    - [ ] c\n      - [ ] d\n  - [ ] b\n"
    )]
    #[case::crlf("- [ ] a\r\n- [ ] b\r\n", "b", "- [ ] b\r\n- [ ] a\r\n")]
    fn move_up_swaps_with_previous_sibling(
        #[case] text: &str,
        #[case] reference: &str,
        #[case] expected: &str,
    ) {
        let system_under_test = apply(text, TodoDocument::move_up, reference);

        assert_eq!(
            system_under_test,
            (EditOutcome::Changed, expected.to_owned())
        );
    }

    #[rstest]
    #[case::first_sibling("- [ ] a\n- [ ] b\n- [ ] c\n", "a", "- [ ] b\n- [ ] a\n- [ ] c\n")]
    #[case::block_to_last_line_without_newline(
        "- [ ] a\n\t- [ ] a1\n- [ ] b",
        "a",
        "- [ ] b\n- [ ] a\n\t- [ ] a1"
    )]
    #[case::nested_blocks_of_different_length(
        TREE,
        "b",
        concat!(
            "# List\n- [ ] a\n  - [ ] d\n\n  - [ ] b\n    continuation of b\n\n    - [ ] c\n",
            "\n- [ ] e\nParagraph\n- [ ] f",
        )
    )]
    #[case::mixed_indentation(
        "- [ ] a\n    - [ ] b\n  - [ ] c\n    - [ ] d\n",
        "b",
        "- [ ] a\n    - [ ] c\n      - [ ] d\n  - [ ] b\n"
    )]
    fn move_down_swaps_with_next_sibling(
        #[case] text: &str,
        #[case] reference: &str,
        #[case] expected: &str,
    ) {
        let system_under_test = apply(text, TodoDocument::move_down, reference);

        assert_eq!(
            system_under_test,
            (EditOutcome::Changed, expected.to_owned())
        );
    }

    #[rstest]
    #[case::leaf_with_fallback(
        "- [ ] a\n- [ ] b\n  text\n",
        "b",
        "- [ ] a\n   - [ ] b\n     text\n"
    )]
    #[case::below_last_child(
        "- [ ] a\n\t- [ ] x\n- [ ] b\n",
        "b",
        "- [ ] a\n\t- [ ] x\n\t- [ ] b\n"
    )]
    #[case::nested_block_with_document_style(
        "- [ ] a\n- [ ] b\n  - [ ] c\n    cont\n",
        "b",
        "- [ ] a\n  - [ ] b\n    - [ ] c\n      cont\n"
    )]
    #[case::keeps_mixed_tabs_and_spaces(
        "\t- [ ] a\n\t- [ ] b\n\t  cont\n      odd\n",
        "b",
        "\t- [ ] a\n\t\t- [ ] b\n\t\t  cont\n\t\t  odd\n"
    )]
    #[case::leaves_raw_lines_between(
        "- [ ] a\n\nNotes\n- [ ] b\n",
        "b",
        "- [ ] a\n\nNotes\n   - [ ] b\n"
    )]
    #[case::middle_sibling_in_tree(
        TREE,
        "d",
        concat!(
            "# List\n- [ ] a\n  - [ ] b\n    continuation of b\n\n    - [ ] c\n\n    - [ ] d\n",
            "\n- [ ] e\nParagraph\n- [ ] f",
        )
    )]
    fn indent_makes_block_last_child_of_previous_sibling(
        #[case] text: &str,
        #[case] reference: &str,
        #[case] expected: &str,
    ) {
        let system_under_test = apply(text, indent_with_three_spaces, reference);

        assert_eq!(
            system_under_test,
            (EditOutcome::Changed, expected.to_owned())
        );
    }

    #[rstest]
    #[case::block_with_continuation(
        "- [ ] a\n  - [ ] b\n    - [ ] c\n    cont\n\n  - [ ] d\n",
        "b",
        "- [ ] a\n- [ ] b\n  - [ ] c\n  cont\n\n  - [ ] d\n"
    )]
    #[case::keeps_mixed_tabs_and_spaces(
        "\t- [ ] a\n\t  - [ ] b\n\t    text\n",
        "b",
        "\t- [ ] a\n\t- [ ] b\n\t  text\n"
    )]
    #[case::grandchild(
        "- [ ] a\n\t- [ ] b\n\t\t- [ ] c\n",
        "c",
        "- [ ] a\n\t- [ ] b\n\t- [ ] c\n"
    )]
    fn outdent_gives_block_indentation_of_parent(
        #[case] text: &str,
        #[case] reference: &str,
        #[case] expected: &str,
    ) {
        let system_under_test = apply(text, TodoDocument::outdent, reference);

        assert_eq!(
            system_under_test,
            (EditOutcome::Changed, expected.to_owned())
        );
    }

    #[rstest]
    #[case::move_up_first_sibling(TodoDocument::move_up, "a")]
    #[case::move_up_first_child(TodoDocument::move_up, "b")]
    #[case::move_up_first_grandchild(TodoDocument::move_up, "c")]
    #[case::move_down_last_sibling(TodoDocument::move_down, "f")]
    #[case::move_down_last_child(TodoDocument::move_down, "d")]
    #[case::move_down_only_grandchild(TodoDocument::move_down, "c")]
    #[case::indent_first_sibling(indent_with_three_spaces, "a")]
    #[case::indent_first_child(indent_with_three_spaces, "b")]
    #[case::outdent_top_level(TodoDocument::outdent, "e")]
    fn edge_of_siblings_is_unchanged(#[case] operation: Operation, #[case] reference: &str) {
        let system_under_test = apply(TREE, operation, reference);

        assert_eq!(system_under_test, (EditOutcome::Unchanged, TREE.to_owned()));
    }

    #[rstest]
    #[case::move_up(TodoDocument::move_up)]
    #[case::move_down(TodoDocument::move_down)]
    #[case::indent(indent_with_three_spaces)]
    #[case::outdent(TodoDocument::outdent)]
    fn unknown_task_is_unchanged(#[case] operation: Operation) {
        let mut document = TodoDocument::parse(TREE);

        let system_under_test = operation(&mut document, UNKNOWN);

        assert_eq!(system_under_test, EditOutcome::Unchanged);
        assert_eq!(document.to_string(), TREE);
    }

    #[rstest]
    #[case::move_up_mixed_indentation(
        "- [ ] a\n    - [ ] b\n  - [ ] c\n    - [ ] d\n",
        TodoDocument::move_up,
        "c",
        vec![("a", None), ("c", Some("a")), ("d", Some("c")), ("b", Some("a"))]
    )]
    #[case::indent_becomes_last_child(
        "- [ ] a\n    - [ ] x\n  - [ ] y\n- [ ] b\n",
        indent_with_three_spaces,
        "b",
        vec![("a", None), ("x", Some("a")), ("y", Some("a")), ("b", Some("a"))]
    )]
    #[case::outdent_adopts_later_siblings(
        "- [ ] a\n  - [ ] b\n  - [ ] c\n",
        TodoDocument::outdent,
        "b",
        vec![("a", None), ("b", None), ("c", Some("b"))]
    )]
    fn operations_produce_expected_tree(
        #[case] text: &str,
        #[case] operation: Operation,
        #[case] reference: &str,
        #[case] expected: Vec<(&str, Option<&str>)>,
    ) {
        let mut document = TodoDocument::parse(text);
        let id = id_of(&document, reference);
        let outcome = operation(&mut document, id);
        let expected: Vec<(String, Option<String>)> = expected
            .into_iter()
            .map(|(task, parent)| (task.to_owned(), parent.map(str::to_owned)))
            .collect();

        let system_under_test = outline(&document);

        assert_eq!(outcome, EditOutcome::Changed);
        assert_eq!(system_under_test, expected);
    }

    type SelectionOperation = fn(&mut TodoDocument, &HashSet<TaskId>) -> EditOutcome;

    fn indent_blocks_with_three_spaces(
        document: &mut TodoDocument,
        ids: &HashSet<TaskId>,
    ) -> EditOutcome {
        document.indent_blocks(ids, spaces(3))
    }

    /// Applies `operation` to the tasks `selected` and returns the outcome and the text.
    fn apply_to_selection(
        text: &str,
        operation: SelectionOperation,
        selected: &[&str],
    ) -> (EditOutcome, String) {
        let mut document = TodoDocument::parse(text);
        let ids = selected
            .iter()
            .map(|description| id_of(&document, description))
            .collect();
        let outcome = operation(&mut document, &ids);
        (outcome, document.to_string())
    }

    #[rstest]
    #[case::siblings_under_the_task_above(
        "- [ ] p\n- [ ] a\n- [ ] b\n- [ ] c\n",
        &["a", "b", "c"],
        "- [ ] p\n   - [ ] a\n   - [ ] b\n   - [ ] c\n"
    )]
    #[case::after_the_last_subtask(
        "- [ ] p\n\t- [ ] x\n- [ ] a\n- [ ] b\n",
        &["a", "b"],
        "- [ ] p\n\t- [ ] x\n\t- [ ] a\n\t- [ ] b\n"
    )]
    #[case::first_run_stays_and_a_later_task_moves(
        "- [ ] a\n- [ ] b\n- [ ] c\n- [ ] d\n",
        &["a", "b", "d"],
        "- [ ] a\n- [ ] b\n- [ ] c\n   - [ ] d\n"
    )]
    #[case::separate_tasks_under_their_own_siblings(
        "- [ ] a\n- [ ] b\n- [ ] c\n- [ ] d\n",
        &["b", "d"],
        "- [ ] a\n   - [ ] b\n- [ ] c\n   - [ ] d\n"
    )]
    #[case::nested_selection_moves_the_block_once(
        "- [ ] p\n- [ ] a\n  - [ ] a1\n- [ ] b\n",
        &["a", "a1", "b"],
        "- [ ] p\n  - [ ] a\n    - [ ] a1\n  - [ ] b\n"
    )]
    #[case::selection_across_depths(
        "- [ ] p\n  - [ ] x\n  - [ ] a\n- [ ] q\n",
        &["a", "q"],
        "- [ ] p\n  - [ ] x\n    - [ ] a\n  - [ ] q\n"
    )]
    fn indent_blocks_keeps_the_relation_of_the_selected_tasks(
        #[case] text: &str,
        #[case] selected: &[&str],
        #[case] expected: &str,
    ) {
        let system_under_test = apply_to_selection(text, indent_blocks_with_three_spaces, selected);

        assert_eq!(
            system_under_test,
            (EditOutcome::Changed, expected.to_owned())
        );
    }

    /// The expected texts are what vim's `<` makes of the selected lines in linewise visual
    /// mode, except that the subtasks of a selected task always move with it.
    #[rstest]
    #[case::siblings(
        "- [ ] a\n  - [ ] b\n  - [ ] c\n",
        &["b", "c"],
        "- [ ] a\n- [ ] b\n- [ ] c\n"
    )]
    #[case::later_sibling_becomes_a_subtask(
        "- [ ] a\n  - [ ] b\n  - [ ] c\n  - [ ] d\n",
        &["b", "c"],
        "- [ ] a\n- [ ] b\n- [ ] c\n  - [ ] d\n"
    )]
    #[case::unselected_subtask_moves_with_its_parent(
        "- [ ] a\n  - [ ] b\n    - [ ] c\n  - [ ] d\n",
        &["b", "d"],
        "- [ ] a\n- [ ] b\n  - [ ] c\n- [ ] d\n"
    )]
    #[case::nested_selection_moves_the_block_once(
        "- [ ] a\n  - [ ] b\n    - [ ] c\n",
        &["b", "c"],
        "- [ ] a\n- [ ] b\n  - [ ] c\n"
    )]
    #[case::top_level_task_stays(
        "- [ ] a\n  - [ ] b\n- [ ] c\n",
        &["b", "c"],
        "- [ ] a\n- [ ] b\n- [ ] c\n"
    )]
    fn outdent_blocks_shifts_every_selected_block(
        #[case] text: &str,
        #[case] selected: &[&str],
        #[case] expected: &str,
    ) {
        let system_under_test = apply_to_selection(text, TodoDocument::outdent_blocks, selected);

        assert_eq!(
            system_under_test,
            (EditOutcome::Changed, expected.to_owned())
        );
    }

    #[rstest]
    #[case::indent_nothing(indent_blocks_with_three_spaces, &[])]
    #[case::indent_the_first_run(indent_blocks_with_three_spaces, &["a", "e"])]
    #[case::indent_first_subtasks(indent_blocks_with_three_spaces, &["b", "d"])]
    #[case::indent_a_parent_with_its_subtasks(indent_blocks_with_three_spaces, &["a", "b", "c"])]
    #[case::outdent_nothing(TodoDocument::outdent_blocks, &[])]
    #[case::outdent_top_level_tasks(TodoDocument::outdent_blocks, &["a", "e", "f"])]
    #[case::outdent_a_parent_with_its_subtasks(TodoDocument::outdent_blocks, &["a", "b", "d"])]
    fn selection_at_the_edge_of_siblings_is_unchanged(
        #[case] operation: SelectionOperation,
        #[case] selected: &[&str],
    ) {
        let system_under_test = apply_to_selection(TREE, operation, selected);

        assert_eq!(system_under_test, (EditOutcome::Unchanged, TREE.to_owned()));
    }

    #[rstest]
    #[case::indent(indent_blocks_with_three_spaces)]
    #[case::outdent(TodoDocument::outdent_blocks)]
    fn unknown_selection_is_unchanged(#[case] operation: SelectionOperation) {
        let mut document = TodoDocument::parse(TREE);
        let ids = HashSet::from([UNKNOWN]);

        let system_under_test = operation(&mut document, &ids);

        assert_eq!(system_under_test, EditOutcome::Unchanged);
        assert_eq!(document.to_string(), TREE);
    }

    #[test]
    fn moved_tasks_keep_their_ids() {
        let mut document = TodoDocument::parse(TREE);
        let b = id_of(&document, "b");
        let c = id_of(&document, "c");
        let d = id_of(&document, "d");
        let a = id_of(&document, "a");
        let expected = vec![a, d, b, c];

        let system_under_test = document.move_up(d);

        assert_eq!(system_under_test, EditOutcome::Changed);
        assert_eq!(
            document.block(a).unwrap().task_ids().collect::<Vec<_>>(),
            expected
        );
    }
}

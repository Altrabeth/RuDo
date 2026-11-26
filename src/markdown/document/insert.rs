//! Inserting, removing, copying and pasting tasks together with their blocks.
//!
//! New lines get the dominant line ending of the document; neighbouring lines are never
//! changed, except that the terminator moves when lines are appended to or removed from the
//! end of a file that has no final newline.

use std::collections::HashSet;
use std::iter;
use std::ops::Range;

use jiff::civil::Date;

use crate::config::IndentStyle;
use crate::markdown::task_line::{FieldKey, InlineField, ListMarker, TaskLine, indentation_width};

use super::{BLANKS, DocumentLine, LineKind, TaskId, TodoDocument};

/// Where [`TodoDocument::insert_task`] and [`TodoDocument::insert_block`] put new lines.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum InsertPosition {
    /// After the whole block of the task, at the same depth.
    Below(TaskId),
    /// Directly before the task, at the same depth.
    Above(TaskId),
    /// After the block of the task, as its last subtask.
    LastChild(TaskId),
    /// After the block of the last top-level task, or at the end of the document if it has no
    /// tasks.
    End,
}

/// A task with its block, detached from any document: the unit of the yank register.
///
/// Indentation is stored relative to the task, so the block can be pasted at any depth of any
/// document with [`TodoDocument::insert_block`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskBlock {
    /// The task the block belongs to, without indentation.
    root: TaskLine,
    /// The following lines of the block.
    descendants: Vec<DetachedLine>,
}

/// A line of a [`TaskBlock`] after its task.
#[derive(Debug, Clone, PartialEq, Eq)]
enum DetachedLine {
    /// A subtask without indentation, `depth` levels below the root task.
    Task { depth: usize, task: TaskLine },
    /// A line that is not a task, with only the indentation beyond the task it belongs to,
    /// which is `depth` levels below the root task.
    Text { depth: usize, text: String },
    /// A blank line, kept verbatim.
    Blank(String),
}

/// Where new lines go and how a new task there starts.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Target {
    line_index: usize,
    indent: String,
    marker: ListMarker,
}

impl TodoDocument {
    /// Inserts an open task with `description` (inline fields in it are extracted) at
    /// `position` and returns its id, or `None` if the task `position` refers to is not in this
    /// document.
    ///
    /// With a `created` date the task gets the field `[created:: YYYY-MM-DD]` after the fields
    /// of `description`, unless `description` already has a `created` field, which is kept.
    ///
    /// The list marker is taken from the sibling the task is inserted next to: the following
    /// number below an ordered item, the same number above it, the same bullet otherwise, and
    /// `-` without a sibling. A first subtask is indented by one level of
    /// [`TodoDocument::indent_style`] with `fallback_indent`.
    ///
    /// # Panics
    ///
    /// Panics if the document already contains [`u32::MAX`] tasks.
    ///
    /// # Examples
    ///
    /// ```
    /// use jiff::civil::date;
    /// use rudo::config::IndentStyle;
    /// use rudo::markdown::document::{InsertPosition, TodoDocument};
    ///
    /// let mut document = TodoDocument::parse("1. [ ] Wash\n");
    /// let wash = document.tasks().next().expect("a task").id();
    /// let today = date(2026, 11, 25);
    /// document.insert_task(InsertPosition::Below(wash), "Dry", Some(today), IndentStyle::Tab);
    /// document.insert_task(InsertPosition::LastChild(wash), "Rinse", None, IndentStyle::Tab);
    ///
    /// assert_eq!(
    ///     document.to_string(),
    ///     "1. [ ] Wash\n\t- [ ] Rinse\n2. [ ] Dry [created:: 2026-11-25]\n"
    /// );
    /// ```
    #[must_use = "the id is the only handle on the new task"]
    pub fn insert_task(
        &mut self,
        position: InsertPosition,
        description: &str,
        created: Option<Date>,
        fallback_indent: IndentStyle,
    ) -> Option<TaskId> {
        let target = self.target(position, fallback_indent)?;
        let mut task = TaskLine::parse(&format!("{}{} [ ]", target.indent, target.marker))
            .expect("indentation, a list marker and an empty checkbox form a task line");
        task.set_description(description);
        if let Some(date) = created
            && task.field(&FieldKey::Created).is_none()
        {
            task.set_field(InlineField::with_date(FieldKey::Created, date));
        }
        let id = self.allocate_task_id();
        self.insert_lines(target.line_index, iter::once(LineKind::Task(id, task)));
        Some(id)
    }

    /// Removes a task with its block and returns it detached, or `None` if `id` is not in this
    /// document.
    #[must_use = "the removed block is lost if it is not kept"]
    pub fn remove_block(&mut self, id: TaskId) -> Option<TaskBlock> {
        let removed = self.remove_lines(self.block_range(id)?);
        Some(TaskBlock::detach(removed.into_iter().map(|line| line.kind)))
    }

    /// Removes the tasks `ids` with their blocks and returns the blocks detached, in document
    /// order.
    ///
    /// A task inside the block of another task of `ids` leaves with that block, so no line is
    /// removed twice; ids that are not in this document are ignored. The blocks are removed
    /// from the bottom of the document to the top, so that removing one never shifts the lines
    /// of another.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::collections::HashSet;
    ///
    /// use rudo::markdown::document::TodoDocument;
    ///
    /// let mut document = TodoDocument::parse("- [ ] a\n\t- [ ] b\n- [ ] c\n- [ ] d\n");
    /// let ids: HashSet<_> = document.tasks().take(3).map(|node| node.id()).collect();
    ///
    /// let removed = document.remove_blocks(&ids);
    ///
    /// assert_eq!(removed.len(), 2);
    /// assert_eq!(document.to_string(), "- [ ] d\n");
    /// ```
    #[must_use = "the removed blocks are lost if they are not kept"]
    pub fn remove_blocks(&mut self, ids: &HashSet<TaskId>) -> Vec<TaskBlock> {
        let mut blocks: Vec<TaskBlock> = self
            .outermost_blocks(ids)
            .into_iter()
            .rev()
            .map(|(_, range)| {
                TaskBlock::detach(self.remove_lines(range).into_iter().map(|line| line.kind))
            })
            .collect();
        blocks.reverse();
        blocks
    }

    /// Returns detached copies of the tasks `ids` with their blocks, in document order; like
    /// [`TodoDocument::remove_blocks`], a task inside the block of another one is copied once,
    /// with that block.
    #[must_use]
    pub fn copy_blocks(&self, ids: &HashSet<TaskId>) -> Vec<TaskBlock> {
        self.outermost_blocks(ids)
            .into_iter()
            .map(|(_, range)| {
                TaskBlock::detach(self.lines[range].iter().map(|line| line.kind.clone()))
            })
            .collect()
    }

    /// Returns the tasks of `ids` that are not inside the block of another task of `ids`, with
    /// the line ranges of their blocks, in document order, in one pass over the lines.
    pub(super) fn outermost_blocks(&self, ids: &HashSet<TaskId>) -> Vec<(TaskId, Range<usize>)> {
        let mut blocks = Vec::new();
        let mut index = 0;
        while index < self.lines.len() {
            match self.lines[index].task() {
                Some((id, _)) if ids.contains(&id) => {
                    let end = index + self.block_at(index).lines.len();
                    blocks.push((id, index..end));
                    index = end;
                }
                Some(_) | None => index += 1,
            }
        }
        blocks
    }

    /// Returns a detached copy of a task with its block, or `None` if `id` is not in this
    /// document.
    #[must_use]
    pub fn copy_block(&self, id: TaskId) -> Option<TaskBlock> {
        let block = self.block(id)?;
        Some(TaskBlock::detach(
            block.lines.iter().map(|line| line.kind.clone()),
        ))
    }

    /// Pastes a block at `position` and returns the id of its task, or `None` if the task
    /// `position` refers to is not in this document.
    ///
    /// The block is re-indented to its new depth with [`TodoDocument::indent_style`] (using
    /// `fallback_indent`), and every task in it gets a fresh id.
    ///
    /// # Panics
    ///
    /// Panics if the document would contain more than [`u32::MAX`] tasks.
    ///
    /// # Examples
    ///
    /// ```
    /// use rudo::config::IndentStyle;
    /// use rudo::markdown::document::{InsertPosition, TodoDocument};
    ///
    /// let mut source = TodoDocument::parse("- [ ] Trip\n\t- [ ] Pack\n");
    /// let trip = source.tasks().next().expect("a task").id();
    /// let block = source.remove_block(trip).expect("trip is in the document");
    ///
    /// let mut target = TodoDocument::parse("- [ ] Summer\n  - [ ] Swim\n");
    /// let summer = target.tasks().next().expect("a task").id();
    /// target.insert_block(&block, InsertPosition::LastChild(summer), IndentStyle::Tab);
    ///
    /// assert_eq!(source.to_string(), "");
    /// assert_eq!(
    ///     target.to_string(),
    ///     "- [ ] Summer\n  - [ ] Swim\n  - [ ] Trip\n    - [ ] Pack\n"
    /// );
    /// ```
    #[must_use = "the id is the only handle on the pasted task"]
    pub fn insert_block(
        &mut self,
        block: &TaskBlock,
        position: InsertPosition,
        fallback_indent: IndentStyle,
    ) -> Option<TaskId> {
        let target = self.target(position, fallback_indent)?;
        let unit = indent_unit(self.indent_style(fallback_indent));
        let root_id = self.allocate_task_id();
        let descendants: Vec<LineKind> = block
            .descendants
            .iter()
            .map(|line| line.attach(&target.indent, &unit, self))
            .collect();
        let mut root = block.root.clone();
        root.set_indent(target.indent);
        let lines = iter::once(LineKind::Task(root_id, root)).chain(descendants);
        self.insert_lines(target.line_index, lines);
        Some(root_id)
    }

    /// Resolves `position` to the line index of new lines, their indentation and the list
    /// marker of a new task there.
    fn target(&self, position: InsertPosition, fallback_indent: IndentStyle) -> Option<Target> {
        match position {
            InsertPosition::Below(id) => {
                let task = self.task_line(id)?;
                Some(Target {
                    line_index: self.block_end(id)?,
                    indent: task.indent().to_owned(),
                    marker: task.marker().successor(),
                })
            }
            InsertPosition::Above(id) => {
                let task = self.task_line(id)?;
                Some(Target {
                    line_index: self.line_index(id)?,
                    indent: task.indent().to_owned(),
                    marker: task.marker(),
                })
            }
            InsertPosition::LastChild(id) => match self.children(id).last() {
                Some(last_child) => self.target(InsertPosition::Below(last_child), fallback_indent),
                None => Some(Target {
                    line_index: self.block_end(id)?,
                    indent: self.child_indent(id, fallback_indent)?,
                    marker: ListMarker::Dash,
                }),
            },
            InsertPosition::End => match self.tasks().filter(|node| node.depth() == 0).last() {
                Some(last) => self.target(InsertPosition::Below(last.id()), fallback_indent),
                None => Some(Target {
                    line_index: self.lines.len(),
                    indent: String::new(),
                    marker: ListMarker::Dash,
                }),
            },
        }
    }

    /// Returns the indentation of a new last subtask of the task `id`: that of its last
    /// subtask, or one level of [`TodoDocument::indent_style`] beyond the task without one.
    pub(super) fn child_indent(&self, id: TaskId, fallback_indent: IndentStyle) -> Option<String> {
        match self.children(id).last() {
            Some(last_child) => Some(self.task_line(last_child)?.indent().to_owned()),
            None => Some(
                self.task_line(id)?.indent().to_owned()
                    + &indent_unit(self.indent_style(fallback_indent)),
            ),
        }
    }

    pub(super) fn task_line(&self, id: TaskId) -> Option<&TaskLine> {
        let (_, task) = self.lines[self.line_index(id)?].task()?;
        Some(task)
    }

    /// Returns the line indices of the block of a task.
    pub(super) fn block_range(&self, id: TaskId) -> Option<Range<usize>> {
        let start = self.line_index(id)?;
        Some(start..start + self.block(id)?.lines.len())
    }

    /// Returns the index of the first line after the block of a task.
    fn block_end(&self, id: TaskId) -> Option<usize> {
        Some(self.block_range(id)?.end)
    }

    /// Inserts lines before `index` with the dominant line ending. Lines appended to a file
    /// without a final newline take over the missing terminator.
    fn insert_lines(&mut self, index: usize, kinds: impl IntoIterator<Item = LineKind>) {
        let ending = Some(self.line_ending);
        let appends_to_unterminated_line = index == self.lines.len()
            && self.lines.last().is_some_and(|line| line.ending.is_none());
        self.lines.splice(
            index..index,
            kinds.into_iter().map(|kind| DocumentLine { kind, ending }),
        );
        if appends_to_unterminated_line {
            self.lines[index - 1].ending = ending;
            self.lines
                .last_mut()
                .expect("the document has a line after index")
                .ending = None;
        }
    }

    /// Removes the lines in `range` and returns them. If they end a file without a final
    /// newline, the new last line loses its terminator.
    fn remove_lines(&mut self, range: Range<usize>) -> Vec<DocumentLine> {
        let removes_last_line = range.end == self.lines.len();
        let removed: Vec<DocumentLine> = self.lines.drain(range).collect();
        let was_unterminated = removed.last().is_some_and(|line| line.ending.is_none());
        if removes_last_line
            && was_unterminated
            && let Some(last) = self.lines.last_mut()
        {
            last.ending = None;
        }
        removed
    }
}

impl TaskBlock {
    /// Returns the number of tasks in the block, counting its task and every subtask.
    #[must_use]
    pub fn task_count(&self) -> usize {
        1 + self
            .descendants
            .iter()
            .filter(|line| matches!(line, DetachedLine::Task { .. }))
            .count()
    }

    /// Detaches the lines of a block, which start with its task.
    fn detach(mut kinds: impl Iterator<Item = LineKind>) -> Self {
        let Some(LineKind::Task(_, mut root)) = kinds.next() else {
            unreachable!("a block starts with its task");
        };
        let mut ancestors = vec![Ancestor::of(&root)];
        root.set_indent(String::new());
        let descendants = kinds
            .map(|kind| DetachedLine::detach(kind, &mut ancestors))
            .collect();
        Self { root, descendants }
    }
}

/// A task above the current line of a block, for computing relative indentation.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Ancestor {
    width: usize,
    indent: String,
}

impl Ancestor {
    fn of(task: &TaskLine) -> Self {
        Self {
            width: task.indent_width(),
            indent: task.indent().to_owned(),
        }
    }
}

/// Returns the position of the innermost ancestor indented less than `width`, which a line of
/// that width belongs to. The root task (position 0) is indented less than every line after
/// it in its block.
fn owner_position(ancestors: &[Ancestor], width: usize) -> usize {
    ancestors
        .iter()
        .rposition(|ancestor| ancestor.width < width)
        .unwrap_or(0)
}

impl DetachedLine {
    /// Detaches one line of a block. `ancestors` holds the tasks above it from the root task
    /// inwards and is updated when the line is a task.
    fn detach(kind: LineKind, ancestors: &mut Vec<Ancestor>) -> Self {
        match kind {
            LineKind::Task(_, mut task) => {
                let owner = owner_position(ancestors, task.indent_width());
                ancestors.truncate(owner + 1);
                ancestors.push(Ancestor::of(&task));
                task.set_indent(String::new());
                Self::Task {
                    depth: owner + 1,
                    task,
                }
            }
            LineKind::Raw(text) if text.trim().is_empty() => Self::Blank(text),
            LineKind::Raw(mut text) => {
                let depth = owner_position(ancestors, indentation_width(&text));
                let owner = &ancestors[depth];
                replace_indent(&mut text, &owner.indent, owner.width, "");
                Self::Text { depth, text }
            }
        }
    }

    /// Returns the line for a block whose task is indented by `base`, with `unit` per level.
    /// Tasks get fresh ids from `document`.
    fn attach(&self, base: &str, unit: &str, document: &mut TodoDocument) -> LineKind {
        let indent = |depth: usize| base.to_owned() + &unit.repeat(depth);
        match self {
            Self::Task { depth, task } => {
                let mut task = task.clone();
                task.set_indent(indent(*depth));
                LineKind::Task(document.allocate_task_id(), task)
            }
            Self::Text { depth, text } => LineKind::Raw(indent(*depth) + text),
            Self::Blank(text) => LineKind::Raw(text.clone()),
        }
    }
}

/// Replaces the indentation `old`, `old_width` columns wide, at the start of `text` with `new`.
///
/// Lines that do not start with `old`, because they mix tabs and spaces differently, keep
/// the columns beyond `old_width` as spaces after `new`.
pub(super) fn replace_indent(text: &mut String, old: &str, old_width: usize, new: &str) {
    if text.starts_with(old) {
        text.replace_range(..old.len(), new);
    } else {
        let indent_length = text.len() - text.trim_start_matches(BLANKS).len();
        let extra_width = indentation_width(text).saturating_sub(old_width);
        text.replace_range(
            ..indent_length,
            &(new.to_owned() + &" ".repeat(extra_width)),
        );
    }
}

/// Returns the indentation of one level.
fn indent_unit(style: IndentStyle) -> String {
    match style {
        IndentStyle::Tab => "\t".to_owned(),
        IndentStyle::Spaces(width) => " ".repeat(usize::from(width.get())),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use jiff::civil::date;
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

    fn spaces(width: u8) -> IndentStyle {
        IndentStyle::Spaces(IndentWidth::new(width).unwrap())
    }

    /// Returns the id of the first task with `description`.
    fn id_of(document: &TodoDocument, description: &str) -> TaskId {
        document
            .tasks()
            .find(|node| node.task().description() == description)
            .unwrap()
            .id()
    }

    fn task_ids(document: &TodoDocument) -> Vec<TaskId> {
        document.tasks().map(|node| node.id()).collect()
    }

    /// Inserts a task `new` relative to the task `reference`.
    fn insert_new(text: &str, position: fn(TaskId) -> InsertPosition, reference: &str) -> String {
        let mut document = TodoDocument::parse(text);
        let position = position(id_of(&document, reference));
        document
            .insert_task(position, "new", None, spaces(3))
            .unwrap();
        document.to_string()
    }

    /// Copies the block of `reference` in `source` and pastes it into `target` at `position`
    /// relative to the task `anchor`.
    fn paste(
        source: &str,
        reference: &str,
        target: &str,
        position: fn(TaskId) -> InsertPosition,
        anchor: &str,
    ) -> String {
        let source = TodoDocument::parse(source);
        let block = source.copy_block(id_of(&source, reference)).unwrap();
        let mut document = TodoDocument::parse(target);
        let position = position(id_of(&document, anchor));
        document.insert_block(&block, position, spaces(3)).unwrap();
        document.to_string()
    }

    #[rstest]
    #[case::below_whole_block(
        InsertPosition::Below,
        "a",
        concat!(
            "# List\n- [ ] a\n  - [ ] b\n    continuation of b\n\n    - [ ] c\n\n  - [ ] d\n",
            "- [ ] new\n\n- [ ] e\nParagraph\n- [ ] f",
        )
    )]
    #[case::below_nested(
        InsertPosition::Below,
        "b",
        concat!(
            "# List\n- [ ] a\n  - [ ] b\n    continuation of b\n\n    - [ ] c\n  - [ ] new\n",
            "\n  - [ ] d\n\n- [ ] e\nParagraph\n- [ ] f",
        )
    )]
    #[case::above(
        InsertPosition::Above,
        "d",
        concat!(
            "# List\n- [ ] a\n  - [ ] b\n    continuation of b\n\n    - [ ] c\n\n  - [ ] new\n",
            "  - [ ] d\n\n- [ ] e\nParagraph\n- [ ] f",
        )
    )]
    #[case::child_of_task_with_children(
        InsertPosition::LastChild,
        "a",
        concat!(
            "# List\n- [ ] a\n  - [ ] b\n    continuation of b\n\n    - [ ] c\n\n  - [ ] d\n",
            "  - [ ] new\n\n- [ ] e\nParagraph\n- [ ] f",
        )
    )]
    #[case::child_of_leaf_uses_document_indent(
        InsertPosition::LastChild,
        "c",
        concat!(
            "# List\n- [ ] a\n  - [ ] b\n    continuation of b\n\n    - [ ] c\n      - [ ] new\n",
            "\n  - [ ] d\n\n- [ ] e\nParagraph\n- [ ] f",
        )
    )]
    #[case::below_last_line_keeps_missing_newline(
        InsertPosition::Below,
        "f",
        concat!(
            "# List\n- [ ] a\n  - [ ] b\n    continuation of b\n\n    - [ ] c\n\n  - [ ] d\n",
            "\n- [ ] e\nParagraph\n- [ ] f\n- [ ] new",
        )
    )]
    fn inserts_task_in_tree(
        #[case] position: fn(TaskId) -> InsertPosition,
        #[case] reference: &str,
        #[case] expected: &str,
    ) {
        let system_under_test = insert_new(TREE, position, reference);

        assert_eq!(system_under_test, expected);
    }

    #[rstest]
    #[case::empty_document("", "- [ ] new\n")]
    #[case::only_prose("# Title\nText", "# Title\nText\n- [ ] new")]
    #[case::trailing_prose("- [ ] a\n\nNotes\n", "- [ ] a\n- [ ] new\n\nNotes\n")]
    #[case::after_last_top_level_block(
        "- [ ] a\n- [ ] b\n\t- [ ] c\n\ttext\n",
        "- [ ] a\n- [ ] b\n\t- [ ] c\n\ttext\n- [ ] new\n"
    )]
    #[case::indented_top_level("  * [ ] a\n", "  * [ ] a\n  * [ ] new\n")]
    #[case::crlf("- [ ] a\r\n", "- [ ] a\r\n- [ ] new\r\n")]
    fn inserts_task_at_end(#[case] text: &str, #[case] expected: &str) {
        let mut system_under_test = TodoDocument::parse(text);

        assert!(
            system_under_test
                .insert_task(InsertPosition::End, "new", None, spaces(3))
                .is_some()
        );
        assert_eq!(system_under_test.to_string(), expected);
    }

    #[rstest]
    #[case::ordered_below("3) [x] a\n", InsertPosition::Below, "a", "3) [x] a\n4) [ ] new\n")]
    #[case::ordered_above("3. [ ] a\n", InsertPosition::Above, "a", "3. [ ] new\n3. [ ] a\n")]
    #[case::bullet_below("+ [ ] a\n", InsertPosition::Below, "a", "+ [ ] a\n+ [ ] new\n")]
    #[case::after_last_child(
        "- [ ] a\n\t1. [ ] b\n",
        InsertPosition::LastChild,
        "a",
        "- [ ] a\n\t1. [ ] b\n\t2. [ ] new\n"
    )]
    #[case::first_child_is_dash(
        "1. [ ] a\n",
        InsertPosition::LastChild,
        "a",
        "1. [ ] a\n   - [ ] new\n"
    )]
    fn copies_marker_from_sibling(
        #[case] text: &str,
        #[case] position: fn(TaskId) -> InsertPosition,
        #[case] reference: &str,
        #[case] expected: &str,
    ) {
        let system_under_test = insert_new(text, position, reference);

        assert_eq!(system_under_test, expected);
    }

    #[test]
    fn inserted_task_has_fresh_id_and_extracted_fields() {
        let mut document = TodoDocument::parse("- [ ] a\n- [ ] b\n");
        let existing: HashSet<TaskId> = task_ids(&document).into_iter().collect();
        let expected = "- [ ] a\n- [ ] Buy milk [due:: 2026-12-01]\n- [ ] b\n";

        let system_under_test = document
            .insert_task(
                InsertPosition::Above(id_of(&document, "b")),
                "Buy [due:: 2026-12-01] milk",
                None,
                spaces(2),
            )
            .unwrap();

        assert!(!existing.contains(&system_under_test));
        assert_eq!(document.to_string(), expected);
    }

    #[rstest]
    #[case::after_the_typed_fields(
        "Buy [due:: 2026-12-01] milk",
        "- [ ] Buy milk [due:: 2026-12-01] [created:: 2026-11-25]\n"
    )]
    #[case::not_over_a_typed_creation_date(
        "Buy milk [created:: 2026-01-02] [due:: 2026-12-01]",
        "- [ ] Buy milk [created:: 2026-01-02] [due:: 2026-12-01]\n"
    )]
    fn inserted_task_gets_the_creation_date(#[case] text: &str, #[case] expected: &str) {
        let mut document = TodoDocument::parse("");
        let created = date(2026, 11, 25);
        let _id = document
            .insert_task(InsertPosition::End, text, Some(created), spaces(2))
            .unwrap();

        let system_under_test = document.to_string();

        assert_eq!(system_under_test, expected);
    }

    #[rstest]
    #[case::below(InsertPosition::Below(UNKNOWN))]
    #[case::above(InsertPosition::Above(UNKNOWN))]
    #[case::last_child(InsertPosition::LastChild(UNKNOWN))]
    fn unknown_reference_task_inserts_nothing(#[case] position: InsertPosition) {
        let mut document = TodoDocument::parse(TREE);
        let block = document.copy_block(id_of(&document, "a")).unwrap();

        let system_under_test = (
            document.insert_task(position, "new", None, spaces(2)),
            document.insert_block(&block, position, spaces(2)),
        );

        assert_eq!(system_under_test, (None, None));
        assert_eq!(document.to_string(), TREE);
    }

    #[rstest]
    #[case::with_continuation_and_inner_blank(
        "b",
        "# List\n- [ ] a\n\n  - [ ] d\n\n- [ ] e\nParagraph\n- [ ] f"
    )]
    #[case::last_line_without_newline(
        "f",
        concat!(
            "# List\n- [ ] a\n  - [ ] b\n    continuation of b\n\n    - [ ] c\n\n  - [ ] d\n",
            "\n- [ ] e\nParagraph",
        )
    )]
    #[case::whole_tree("a", "# List\n\n- [ ] e\nParagraph\n- [ ] f")]
    fn removes_block(#[case] reference: &str, #[case] expected: &str) {
        let mut document = TodoDocument::parse(TREE);
        let removed = document.remove_block(id_of(&document, reference)).unwrap();

        let system_under_test = document;

        assert_eq!(system_under_test.to_string(), expected);
        assert_eq!(removed.root.description(), reference);
    }

    /// Returns the ids of the tasks of `document` with one of the `descriptions`.
    fn ids_of(document: &TodoDocument, descriptions: &str) -> HashSet<TaskId> {
        descriptions
            .chars()
            .map(|description| id_of(document, &description.to_string()))
            .collect()
    }

    #[rstest]
    #[case::one(
        "d",
        "d",
        "# List\n- [ ] a\n  - [ ] b\n    continuation of b\n\n    - [ ] c\n\n\n- [ ] e\nParagraph\n- [ ] f"
    )]
    #[case::separated_blocks(
        "ce",
        "ce",
        "# List\n- [ ] a\n  - [ ] b\n    continuation of b\n\n\n  - [ ] d\n\nParagraph\n- [ ] f"
    )]
    #[case::subtask_with_its_parent("abd", "a", "# List\n\n- [ ] e\nParagraph\n- [ ] f")]
    #[case::last_line_without_newline(
        "ef",
        "ef",
        "# List\n- [ ] a\n  - [ ] b\n    continuation of b\n\n    - [ ] c\n\n  - [ ] d\n\nParagraph"
    )]
    fn removes_blocks_from_the_bottom_up_once_each(
        #[case] selected: &str,
        #[case] expected_roots: &str,
        #[case] expected: &str,
    ) {
        let mut document = TodoDocument::parse(TREE);
        let ids = ids_of(&document, selected);
        let removed = document.remove_blocks(&ids);

        let system_under_test = document;

        assert_eq!(system_under_test.to_string(), expected);
        let roots: String = removed
            .iter()
            .map(|block| block.root.description())
            .collect();
        assert_eq!(roots, expected_roots);
    }

    #[test]
    fn copied_blocks_equal_the_removed_ones_and_leave_the_document() {
        let mut document = TodoDocument::parse(TREE);
        let ids = ids_of(&document, "bcf");
        let copied = document.copy_blocks(&ids);
        let expected_count = 3;

        let system_under_test = document.remove_blocks(&ids);

        assert_eq!(system_under_test, copied);
        assert_eq!(
            copied.iter().map(TaskBlock::task_count).sum::<usize>(),
            expected_count
        );
    }

    #[test]
    fn removing_blocks_of_unknown_tasks_changes_nothing() {
        let mut document = TodoDocument::parse(TREE);
        let ids = HashSet::from([UNKNOWN]);

        let system_under_test = document.remove_blocks(&ids);

        assert_eq!(system_under_test, []);
        assert_eq!(document.to_string(), TREE);
    }

    #[test]
    fn removing_only_task_leaves_empty_document() {
        let mut document = TodoDocument::parse("- [ ] a");
        let removed = document.remove_block(id_of(&document, "a")).unwrap();

        let system_under_test = document;

        assert_eq!(system_under_test.to_string(), "");
        assert_eq!(removed.root.to_string(), "- [ ] a");
    }

    #[test]
    fn removing_unknown_task_returns_none() {
        let mut document = TodoDocument::parse(TREE);

        let system_under_test = document.remove_block(UNKNOWN);

        assert_eq!(system_under_test, None);
        assert_eq!(document.to_string(), TREE);
    }

    #[test]
    fn copy_stores_indentation_relative_to_task() {
        let document =
            TodoDocument::parse("- [ ] a\n\t- [ ] b\n\t  text\n      odd\n\n\t\t- [ ] c\n");
        let expected_root = "- [ ] b";
        let expected_descendants = vec![
            DetachedLine::Text {
                depth: 0,
                text: "  text".to_owned(),
            },
            DetachedLine::Text {
                depth: 0,
                text: "  odd".to_owned(),
            },
            DetachedLine::Blank(String::new()),
            DetachedLine::Task {
                depth: 1,
                task: TaskLine::parse("- [ ] c").unwrap(),
            },
        ];

        let system_under_test = document.copy_block(id_of(&document, "b")).unwrap();

        assert_eq!(system_under_test.root.to_string(), expected_root);
        assert_eq!(system_under_test.descendants, expected_descendants);
    }

    #[test]
    fn copy_does_not_change_document() {
        let document = TodoDocument::parse(TREE);

        let system_under_test = document.copy_block(id_of(&document, "a"));

        assert!(system_under_test.is_some());
        assert_eq!(document.to_string(), TREE);
        assert_eq!(document.copy_block(UNKNOWN), None);
    }

    #[rstest]
    #[case::with_subtasks_and_text("a", 4)]
    #[case::subtask_with_continuation("b", 2)]
    #[case::single_task("f", 1)]
    fn task_count_counts_the_task_and_its_subtasks(
        #[case] description: &str,
        #[case] expected: usize,
    ) {
        let document = TodoDocument::parse(TREE);
        let block = document.copy_block(id_of(&document, description)).unwrap();

        let system_under_test = block.task_count();

        assert_eq!(system_under_test, expected);
    }

    /// A tab-indented block with a continuation line, a misaligned line and a blank line.
    const TAB_BLOCK: &str = "- [ ] a\n\t- [ ] b\n\t  cont\n      odd\n\n\t\t- [ ] c\n- [ ] z\n";

    #[rstest]
    #[case::top_level(
        InsertPosition::Below,
        "x",
        "- [ ] x\n  - [ ] y\n- [ ] a\n  - [ ] b\n    cont\n    odd\n\n    - [ ] c\n"
    )]
    #[case::as_last_child(
        InsertPosition::LastChild,
        "x",
        "- [ ] x\n  - [ ] y\n  - [ ] a\n    - [ ] b\n      cont\n      odd\n\n      - [ ] c\n"
    )]
    #[case::above_nested(
        InsertPosition::Above,
        "y",
        "- [ ] x\n  - [ ] a\n    - [ ] b\n      cont\n      odd\n\n      - [ ] c\n  - [ ] y\n"
    )]
    fn pastes_tab_block_into_two_space_document(
        #[case] position: fn(TaskId) -> InsertPosition,
        #[case] anchor: &str,
        #[case] expected: &str,
    ) {
        let system_under_test = paste(TAB_BLOCK, "a", "- [ ] x\n  - [ ] y\n", position, anchor);

        assert_eq!(system_under_test, expected);
    }

    #[rstest]
    #[case::crlf(
        "- [ ] x\r\n",
        InsertPosition::Below,
        "- [ ] x\r\n- [ ] b\r\n  cont\r\n  odd\r\n\r\n   - [ ] c\r\n"
    )]
    #[case::missing_final_newline(
        "- [ ] x",
        InsertPosition::Below,
        "- [ ] x\n- [ ] b\n  cont\n  odd\n\n   - [ ] c"
    )]
    #[case::child_with_fallback_indent(
        "- [ ] x\n",
        InsertPosition::LastChild,
        "- [ ] x\n   - [ ] b\n     cont\n     odd\n\n      - [ ] c\n"
    )]
    fn pastes_with_target_line_endings_and_indent(
        #[case] target: &str,
        #[case] position: fn(TaskId) -> InsertPosition,
        #[case] expected: &str,
    ) {
        let system_under_test = paste(TAB_BLOCK, "b", target, position, "x");

        assert_eq!(system_under_test, expected);
    }

    #[test]
    fn pasted_tasks_get_fresh_ids() {
        let mut document = TodoDocument::parse(TREE);
        let block = document.copy_block(id_of(&document, "a")).unwrap();
        let first = document
            .insert_block(&block, InsertPosition::End, spaces(2))
            .unwrap();
        let second = document
            .insert_block(&block, InsertPosition::Above(first), spaces(2))
            .unwrap();
        let expected_task_count = 14;

        let system_under_test = task_ids(&document);

        assert_ne!(first, second);
        assert_eq!(system_under_test.len(), expected_task_count);
        assert_eq!(
            system_under_test.iter().collect::<HashSet<_>>().len(),
            expected_task_count
        );
    }

    #[rstest]
    #[case::same_prefix("\t  text", "\t", 4, "  ", "    text")]
    #[case::remove_prefix("  - [ ] a", "  ", 2, "", "- [ ] a")]
    #[case::other_mix_keeps_extra_columns_as_spaces("      odd", "\t", 4, "\t\t", "\t\t  odd")]
    #[case::other_mix_without_extra_columns("    x", "\t", 4, "", "x")]
    fn replaces_indent_prefix(
        #[case] text: &str,
        #[case] old: &str,
        #[case] old_width: usize,
        #[case] new: &str,
        #[case] expected: &str,
    ) {
        let mut edited = text.to_owned();
        replace_indent(&mut edited, old, old_width, new);

        let system_under_test = edited;

        assert_eq!(system_under_test, expected);
    }

    #[test]
    fn removed_block_pastes_back_at_another_place() {
        let mut document = TodoDocument::parse(TREE);
        let block = document.remove_block(id_of(&document, "b")).unwrap();
        let position = InsertPosition::Above(id_of(&document, "d"));
        let expected = concat!(
            "# List\n- [ ] a\n\n  - [ ] b\n    continuation of b\n\n    - [ ] c\n  - [ ] d\n",
            "\n- [ ] e\nParagraph\n- [ ] f",
        );
        document.insert_block(&block, position, spaces(7)).unwrap();

        let system_under_test = document.to_string();

        assert_eq!(system_under_test, expected);
    }
}

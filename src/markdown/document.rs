//! A complete list file: every line in order, task lines parsed, everything else verbatim.
//!
//! [`TodoDocument::parse`] never fails and an unedited document renders (with [`Display`])
//! byte for byte as the text it was parsed from: the byte order mark, every line terminator
//! (`\n`, `\r\n` or none on a last line) and every non-task line are kept as they are.
//!
//! Tasks form an implicit tree. A task is the child of the nearest preceding task with a
//! smaller indentation width; lines that are not tasks do not affect the tree. Tasks are
//! addressed by [`TaskId`], so callers never handle line indices.
//!
//! Front matter (a first line `---` up to the next `---` or `...`) and fenced code blocks are
//! never parsed as tasks.

use std::fmt::{self, Display, Formatter};
use std::iter;
use std::slice;

use crate::config::{IndentStyle, IndentWidth};
use crate::markdown::task_line::{TaskLine, indentation_width};

/// The byte order mark, kept when a file starts with it.
const BYTE_ORDER_MARK: char = '\u{feff}';

/// Characters that make up the indentation of a line.
const BLANKS: [char; 2] = [' ', '\t'];

/// Minimum number of backticks or tildes that open a fenced code block.
const MIN_FENCE_LENGTH: usize = 3;

/// Identifies a task within one [`TodoDocument`].
///
/// Ids are assigned when a document is parsed, are unique within it and stay attached to
/// their task while other lines change. Parsing the text again assigns new ids.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TaskId(u32);

/// A line terminator.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, strum::EnumIter)]
pub enum LineEnding {
    /// `\n`, used on Unix.
    #[default]
    Lf,
    /// `\r\n`, used on Windows.
    CrLf,
}

impl LineEnding {
    /// Returns the terminator as text.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Lf => "\n",
            Self::CrLf => "\r\n",
        }
    }
}

/// A parsed Markdown list file.
///
/// # Examples
///
/// ```
/// use rudo::markdown::document::TodoDocument;
///
/// let text = "# Groceries\r\n- [ ] Buy milk\r\n\t- [x] Check lactose-free\r\n";
/// let document = TodoDocument::parse(text);
/// let outline: Vec<(usize, &str)> = document
///     .tasks()
///     .map(|node| (node.depth(), node.task().description()))
///     .collect();
///
/// assert_eq!(outline, [(0, "Buy milk"), (1, "Check lactose-free")]);
/// assert_eq!(document.to_string(), text);
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TodoDocument {
    byte_order_mark: bool,
    lines: Vec<DocumentLine>,
    /// The dominant terminator, for lines inserted later.
    line_ending: LineEnding,
    /// The id that the next new task receives.
    next_task_id: u32,
}

impl TodoDocument {
    /// Parses the complete text of a list file.
    ///
    /// # Panics
    ///
    /// Panics if `text` contains more than [`u32::MAX`] tasks, which takes a file of more
    /// than 20 GiB.
    #[must_use]
    pub fn parse(text: &str) -> Self {
        let (byte_order_mark, text) = match text.strip_prefix(BYTE_ORDER_MARK) {
            Some(rest) => (true, rest),
            None => (false, text),
        };
        let front_matter_lines = front_matter_line_count(text);
        let mut document = Self {
            byte_order_mark,
            ..Self::default()
        };
        let mut fence: Option<Fence> = None;
        for (index, (content, ending)) in split_lines(text).enumerate() {
            let task = (index >= front_matter_lines && !Fence::track(&mut fence, content))
                .then(|| TaskLine::parse(content))
                .flatten();
            let kind = match task {
                Some(task) => LineKind::Task(document.allocate_task_id(), task),
                None => LineKind::Raw(content.to_owned()),
            };
            document.lines.push(DocumentLine { kind, ending });
        }
        document.line_ending = dominant_line_ending(&document.lines);
        document
    }

    /// Returns the terminator used by most lines, [`LineEnding::Lf`] on a tie.
    #[must_use]
    pub fn line_ending(&self) -> LineEnding {
        self.line_ending
    }

    /// Returns the indentation of subtasks used in this document, or `fallback` (the configured
    /// `tasks.indent`) if it has no indented tasks or uses more than [`IndentWidth::MAX`]
    /// spaces per level.
    ///
    /// The first indented task decides between tabs and spaces. With spaces, the width is the
    /// smallest step between a task and its parent (or the left margin for top-level tasks).
    #[must_use]
    pub fn indent_style(&self, fallback: IndentStyle) -> IndentStyle {
        let mut indented = self.tasks().filter(|node| node.indent_step > 0);
        let Some(first) = indented.next() else {
            return fallback;
        };
        if first.task.indent().contains('\t') {
            return IndentStyle::Tab;
        }
        iter::once(first)
            .chain(indented)
            .filter(|node| !node.task.indent().contains('\t'))
            .map(|node| node.indent_step)
            .min()
            .and_then(|step| u8::try_from(step).ok())
            .and_then(IndentWidth::new)
            .map_or(fallback, IndentStyle::Spaces)
    }

    /// Returns all tasks in document order, each with its depth and parent.
    pub fn tasks(&self) -> impl Iterator<Item = TaskNode<'_>> {
        TreeWalk {
            lines: self.lines.iter(),
            ancestors: Vec::new(),
        }
    }

    /// Returns the depth of a task: 0 for a top-level task, or `None` if `id` is not in this
    /// document.
    #[must_use]
    pub fn depth(&self, id: TaskId) -> Option<usize> {
        self.node(id).map(|node| node.depth)
    }

    /// Returns the parent of a task, or `None` if it is a top-level task or `id` is not in
    /// this document.
    #[must_use]
    pub fn parent(&self, id: TaskId) -> Option<TaskId> {
        self.node(id)?.parent
    }

    /// Returns the direct subtasks of a task in document order; none if `id` is not in this
    /// document.
    pub fn children(&self, id: TaskId) -> impl Iterator<Item = TaskId> {
        self.tasks()
            .filter(move |node| node.parent == Some(id))
            .map(|node| node.id)
    }

    /// Returns the block of a task: its line and all following lines that are indented deeper,
    /// including blank lines between them but not trailing blank lines. Returns `None` if `id`
    /// is not in this document.
    #[must_use]
    pub fn block(&self, id: TaskId) -> Option<Block<'_>> {
        let start = self.line_index(id)?;
        let width = self.lines[start].indent_width();
        let following = &self.lines[start + 1..];
        let candidates = following
            .iter()
            .take_while(|line| line.is_blank() || line.indent_width() > width)
            .count();
        let length = following[..candidates]
            .iter()
            .rposition(|line| !line.is_blank())
            .map_or(0, |last| last + 1);
        Some(Block {
            lines: &self.lines[start..=start + length],
        })
    }

    /// Returns the 1-based line number of a task, for opening it in an editor, or `None` if
    /// `id` is not in this document.
    #[must_use]
    pub fn line_number(&self, id: TaskId) -> Option<usize> {
        self.line_index(id).map(|index| index + 1)
    }

    fn node(&self, id: TaskId) -> Option<TaskNode<'_>> {
        self.tasks().find(|node| node.id == id)
    }

    fn line_index(&self, id: TaskId) -> Option<usize> {
        self.lines
            .iter()
            .position(|line| line.task().is_some_and(|(task_id, _)| task_id == id))
    }

    fn allocate_task_id(&mut self) -> TaskId {
        let id = TaskId(self.next_task_id);
        self.next_task_id = self
            .next_task_id
            .checked_add(1)
            .expect("a document holds fewer than u32::MAX tasks");
        id
    }
}

impl Display for TodoDocument {
    /// Writes the document: unedited lines exactly as they were parsed.
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        if self.byte_order_mark {
            write!(formatter, "{BYTE_ORDER_MARK}")?;
        }
        write_lines(formatter, &self.lines)
    }
}

/// A task together with its position in the task tree, as yielded by [`TodoDocument::tasks`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TaskNode<'document> {
    id: TaskId,
    task: &'document TaskLine,
    depth: usize,
    parent: Option<TaskId>,
    /// Indentation width beyond the parent's, or beyond the left margin without a parent.
    indent_step: usize,
}

impl<'document> TaskNode<'document> {
    /// Returns the id of the task.
    #[must_use]
    pub fn id(&self) -> TaskId {
        self.id
    }

    /// Returns the task line.
    #[must_use]
    pub fn task(&self) -> &'document TaskLine {
        self.task
    }

    /// Returns the number of ancestors: 0 for a top-level task.
    #[must_use]
    pub fn depth(&self) -> usize {
        self.depth
    }

    /// Returns the parent, or `None` for a top-level task.
    #[must_use]
    pub fn parent(&self) -> Option<TaskId> {
        self.parent
    }
}

/// The lines of a task and its subtree, as returned by [`TodoDocument::block`].
///
/// It renders (with [`Display`]) as the lines it covers, each with its terminator.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Block<'document> {
    lines: &'document [DocumentLine],
}

impl Block<'_> {
    /// Returns the ids of the tasks in the block in document order, starting with the task the
    /// block belongs to.
    pub fn task_ids(&self) -> impl Iterator<Item = TaskId> {
        self.lines
            .iter()
            .filter_map(|line| line.task().map(|(id, _)| id))
    }
}

impl Display for Block<'_> {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        write_lines(formatter, self.lines)
    }
}

/// One line of a document with its terminator.
#[derive(Debug, Clone, PartialEq, Eq)]
struct DocumentLine {
    kind: LineKind,
    /// `None` only for a last line without a terminator.
    ending: Option<LineEnding>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum LineKind {
    /// Anything that is not a task, kept verbatim.
    Raw(String),
    Task(TaskId, TaskLine),
}

impl DocumentLine {
    fn task(&self) -> Option<(TaskId, &TaskLine)> {
        match &self.kind {
            LineKind::Raw(_) => None,
            LineKind::Task(id, task) => Some((*id, task)),
        }
    }

    fn indent_width(&self) -> usize {
        match &self.kind {
            LineKind::Raw(text) => indentation_width(text),
            LineKind::Task(_, task) => task.indent_width(),
        }
    }

    fn is_blank(&self) -> bool {
        match &self.kind {
            LineKind::Raw(text) => text.trim().is_empty(),
            LineKind::Task(..) => false,
        }
    }
}

impl Display for DocumentLine {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match &self.kind {
            LineKind::Raw(text) => formatter.write_str(text)?,
            LineKind::Task(_, task) => write!(formatter, "{task}")?,
        }
        self.ending
            .map_or(Ok(()), |ending| formatter.write_str(ending.as_str()))
    }
}

/// Walks the tasks of a document in order, tracking the chain of ancestors of the current
/// task so that depths and parents are computed in a single pass.
struct TreeWalk<'document> {
    lines: slice::Iter<'document, DocumentLine>,
    /// Indentation width and id of every ancestor of the next task, outermost first.
    ancestors: Vec<(usize, TaskId)>,
}

impl<'document> Iterator for TreeWalk<'document> {
    type Item = TaskNode<'document>;

    fn next(&mut self) -> Option<Self::Item> {
        let (id, task) = self.lines.find_map(DocumentLine::task)?;
        let width = task.indent_width();
        while self
            .ancestors
            .last()
            .is_some_and(|&(ancestor_width, _)| ancestor_width >= width)
        {
            self.ancestors.pop();
        }
        let parent = self.ancestors.last().copied();
        let node = TaskNode {
            id,
            task,
            depth: self.ancestors.len(),
            parent: parent.map(|(_, parent_id)| parent_id),
            indent_step: width - parent.map_or(0, |(parent_width, _)| parent_width),
        };
        self.ancestors.push((width, id));
        Some(node)
    }
}

/// An open fenced code block: its character and the length of its opening run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Fence {
    character: char,
    length: usize,
}

impl Fence {
    /// Updates the fence state with the next line and returns whether that line belongs to a
    /// fenced code block (an opening fence, a code line or a closing fence).
    fn track(open: &mut Option<Self>, line: &str) -> bool {
        let Some(fence) = open else {
            *open = Self::opened_by(line);
            return open.is_some();
        };
        if fence.is_closed_by(line) {
            *open = None;
        }
        true
    }

    /// Returns the fence that `line` opens: at least three backticks or tildes after optional
    /// indentation. An info string after backticks must not contain a backtick.
    fn opened_by(line: &str) -> Option<Self> {
        let content = line.trim_start_matches(BLANKS);
        let character = content.chars().next().filter(|&c| c == '`' || c == '~')?;
        let info = content.trim_start_matches(character);
        let fence = Self {
            character,
            length: content.len() - info.len(),
        };
        let valid_info = character != '`' || !info.contains('`');
        (fence.length >= MIN_FENCE_LENGTH && valid_info).then_some(fence)
    }

    /// Returns whether `line` closes this fence: at least as many of the same character after
    /// optional indentation, followed only by whitespace.
    fn is_closed_by(self, line: &str) -> bool {
        let content = line.trim_start_matches(BLANKS);
        let rest = content.trim_start_matches(self.character);
        content.len() - rest.len() >= self.length && rest.trim().is_empty()
    }
}

/// Splits `text` into lines without their terminators.
fn split_lines(text: &str) -> impl Iterator<Item = (&str, Option<LineEnding>)> {
    text.split_inclusive('\n').map(|line| {
        if let Some(content) = line.strip_suffix("\r\n") {
            (content, Some(LineEnding::CrLf))
        } else if let Some(content) = line.strip_suffix('\n') {
            (content, Some(LineEnding::Lf))
        } else {
            (line, None)
        }
    })
}

/// Returns the number of lines of the front matter at the start of `text`, including both
/// delimiters, or 0 if there is no closed front matter.
fn front_matter_line_count(text: &str) -> usize {
    let mut lines = split_lines(text).map(|(content, _)| content);
    if lines.next() != Some("---") {
        return 0;
    }
    lines
        .position(|line| line == "---" || line == "...")
        .map_or(0, |closing| closing + 2)
}

fn dominant_line_ending(lines: &[DocumentLine]) -> LineEnding {
    let crlf_count = lines
        .iter()
        .filter(|line| line.ending == Some(LineEnding::CrLf))
        .count();
    let lf_count = lines
        .iter()
        .filter(|line| line.ending == Some(LineEnding::Lf))
        .count();
    if crlf_count > lf_count {
        LineEnding::CrLf
    } else {
        LineEnding::Lf
    }
}

fn write_lines(formatter: &mut Formatter<'_>, lines: &[DocumentLine]) -> fmt::Result {
    lines
        .iter()
        .try_for_each(|line| write!(formatter, "{line}"))
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use proptest::prelude::*;
    use rstest::rstest;
    use strum::IntoEnumIterator;

    use super::*;

    /// A document exercising the tree queries. Ids are assigned from 0 in document order.
    const TREE: &str = concat!(
        "# List\n",                // 1
        "- [ ] a\n",               // 2: TaskId(0)
        "  - [ ] b\n",             // 3: TaskId(1)
        "    continuation of b\n", // 4
        "\n",                      // 5
        "    - [ ] c\n",           // 6: TaskId(2)
        "\n",                      // 7
        "  - [ ] d\n",             // 8: TaskId(3)
        "\n",                      // 9
        "- [ ] e\n",               // 10: TaskId(4)
        "Paragraph\n",             // 11
        "- [ ] f",                 // 12: TaskId(5)
    );

    const UNKNOWN: TaskId = TaskId(99);

    fn descriptions(document: &TodoDocument) -> Vec<&str> {
        document
            .tasks()
            .map(|node| node.task().description())
            .collect()
    }

    fn spaces(width: u8) -> IndentStyle {
        IndentStyle::Spaces(IndentWidth::new(width).unwrap())
    }

    #[rstest]
    #[case::empty("")]
    #[case::only_newline("\n")]
    #[case::no_trailing_newline("- [ ] a\n- [x] b")]
    #[case::crlf("- [ ] a\r\n\r\n- [x] b\r\n")]
    #[case::mixed_endings("a\r\nb\nc\r\n")]
    #[case::lone_carriage_return("- [ ] a\rb\r")]
    #[case::byte_order_mark("\u{feff}- [ ] a\n")]
    #[case::front_matter("---\ntitle: x\n---\n- [ ] a\n")]
    #[case::unclosed_fence("```\n- [ ] a\n")]
    #[case::whitespace_task("  - [ ]   spaced  out  \t\n")]
    fn renders_unedited_document_as_parsed(#[case] text: &str) {
        let system_under_test = TodoDocument::parse(text);

        assert_eq!(system_under_test.to_string(), text);
    }

    #[test]
    fn empty_text_has_no_lines() {
        let text = "";

        let system_under_test = TodoDocument::parse(text);

        assert_eq!(system_under_test.lines, []);
    }

    #[rstest]
    #[case::lf("a\nb", vec![Some(LineEnding::Lf), None])]
    #[case::crlf("a\r\nb\r\n", vec![Some(LineEnding::CrLf), Some(LineEnding::CrLf)])]
    #[case::mixed("a\r\nb\n", vec![Some(LineEnding::CrLf), Some(LineEnding::Lf)])]
    #[case::lone_carriage_return("a\rb", vec![None])]
    fn keeps_terminator_per_line(#[case] text: &str, #[case] expected: Vec<Option<LineEnding>>) {
        let system_under_test = TodoDocument::parse(text);

        assert_eq!(
            system_under_test
                .lines
                .iter()
                .map(|line| line.ending)
                .collect::<Vec<_>>(),
            expected
        );
    }

    #[test]
    fn uniform_line_ending_is_dominant() {
        for ending in LineEnding::iter() {
            let text = ["a", "b", "c"].join(ending.as_str());

            let system_under_test = TodoDocument::parse(&text);

            assert_eq!(system_under_test.line_ending(), ending);
        }
    }

    #[rstest]
    #[case::empty("", LineEnding::Lf)]
    #[case::no_terminator("a", LineEnding::Lf)]
    #[case::mostly_crlf("a\r\nb\r\nc\n", LineEnding::CrLf)]
    #[case::mostly_lf("a\nb\nc\r\n", LineEnding::Lf)]
    #[case::tie("a\nb\r\n", LineEnding::Lf)]
    fn detects_dominant_line_ending(#[case] text: &str, #[case] expected: LineEnding) {
        let system_under_test = TodoDocument::parse(text);

        assert_eq!(system_under_test.line_ending(), expected);
    }

    #[rstest]
    #[case::plain("- [ ] a\n* [x] b\n1. [/] c", vec!["a", "b", "c"])]
    #[case::byte_order_mark("\u{feff}- [ ] a", vec!["a"])]
    #[case::block_quote("> - [ ] quoted\n- [ ] a", vec!["a"])]
    #[case::front_matter("---\n- [ ] hidden\n---\n- [ ] a", vec!["a"])]
    #[case::front_matter_dots("---\n- [ ] hidden\n...\n- [ ] a", vec!["a"])]
    #[case::front_matter_after_bom("\u{feff}---\n- [ ] hidden\n---\n- [ ] a", vec!["a"])]
    #[case::front_matter_crlf("---\r\n- [ ] hidden\r\n---\r\n- [ ] a", vec!["a"])]
    #[case::unclosed_front_matter("---\n- [ ] a", vec!["a"])]
    #[case::rule_not_first("- [ ] a\n---\n- [ ] b\n---", vec!["a", "b"])]
    #[case::backtick_fence("```\n- [ ] hidden\n```\n- [ ] a", vec!["a"])]
    #[case::tilde_fence("~~~ md\n- [ ] hidden\n~~~\n- [ ] a", vec!["a"])]
    #[case::indented_fence("- [ ] a\n  ```\n  - [ ] hidden\n  ```\n- [ ] b", vec!["a", "b"])]
    #[case::longer_closing("````\n- [ ] hidden\n`````\n- [ ] a", vec!["a"])]
    #[case::shorter_run_not_closing("````\n```\n- [ ] hidden\n````\n- [ ] a", vec!["a"])]
    #[case::other_character_not_closing("```\n~~~\n- [ ] hidden\n```\n- [ ] a", vec!["a"])]
    #[case::closing_with_text("```\n``` x\n- [ ] hidden\n```\n- [ ] a", vec!["a"])]
    #[case::unclosed_fence("- [ ] a\n```\n- [ ] hidden", vec!["a"])]
    #[case::two_backticks("``\n- [ ] a", vec!["a"])]
    #[case::inline_code("```code```\n- [ ] a", vec!["a"])]
    fn parses_tasks_outside_front_matter_and_fences(
        #[case] text: &str,
        #[case] expected: Vec<&str>,
    ) {
        let system_under_test = TodoDocument::parse(text);

        assert_eq!(descriptions(&system_under_test), expected);
    }

    #[test]
    fn task_ids_are_unique() {
        let text = "- [ ] a\n\t- [ ] b\n- [ ] c\n";
        let expected_count = 3;

        let system_under_test = TodoDocument::parse(text);

        assert_eq!(
            system_under_test
                .tasks()
                .map(|node| node.id())
                .collect::<HashSet<_>>()
                .len(),
            expected_count
        );
    }

    #[test]
    fn allocated_ids_continue_after_parsed_ones() {
        let mut document = TodoDocument::parse("- [ ] a\n- [ ] b\n");
        let parsed: HashSet<TaskId> = document.tasks().map(|node| node.id()).collect();

        let system_under_test = document.allocate_task_id();

        assert!(!parsed.contains(&system_under_test));
    }

    #[test]
    fn iterates_tasks_with_depth_and_parent() {
        let expected = vec![
            (TaskId(0), 0, None),
            (TaskId(1), 1, Some(TaskId(0))),
            (TaskId(2), 2, Some(TaskId(1))),
            (TaskId(3), 1, Some(TaskId(0))),
            (TaskId(4), 0, None),
            (TaskId(5), 0, None),
        ];

        let system_under_test = TodoDocument::parse(TREE);

        assert_eq!(
            system_under_test
                .tasks()
                .map(|node| (node.id(), node.depth(), node.parent()))
                .collect::<Vec<_>>(),
            expected
        );
    }

    #[rstest]
    #[case::top_level(TaskId(0), Some(0))]
    #[case::child(TaskId(1), Some(1))]
    #[case::grandchild(TaskId(2), Some(2))]
    #[case::sibling_after_grandchild(TaskId(3), Some(1))]
    #[case::unknown(UNKNOWN, None)]
    fn depth_of_task(#[case] id: TaskId, #[case] expected: Option<usize>) {
        let document = TodoDocument::parse(TREE);

        let system_under_test = document.depth(id);

        assert_eq!(system_under_test, expected);
    }

    #[rstest]
    #[case::top_level(TaskId(0), None)]
    #[case::child(TaskId(1), Some(TaskId(0)))]
    #[case::grandchild(TaskId(2), Some(TaskId(1)))]
    #[case::sibling_after_grandchild(TaskId(3), Some(TaskId(0)))]
    #[case::after_paragraph(TaskId(5), None)]
    #[case::unknown(UNKNOWN, None)]
    fn parent_of_task(#[case] id: TaskId, #[case] expected: Option<TaskId>) {
        let document = TodoDocument::parse(TREE);

        let system_under_test = document.parent(id);

        assert_eq!(system_under_test, expected);
    }

    #[rstest]
    #[case::two_children(TaskId(0), vec![TaskId(1), TaskId(3)])]
    #[case::one_child(TaskId(1), vec![TaskId(2)])]
    #[case::leaf(TaskId(2), vec![])]
    #[case::unknown(UNKNOWN, vec![])]
    fn children_of_task(#[case] id: TaskId, #[case] expected: Vec<TaskId>) {
        let document = TodoDocument::parse(TREE);

        let system_under_test: Vec<TaskId> = document.children(id).collect();

        assert_eq!(system_under_test, expected);
    }

    #[rstest]
    #[case::subtree_without_trailing_blank(
        TaskId(0),
        "- [ ] a\n  - [ ] b\n    continuation of b\n\n    - [ ] c\n\n  - [ ] d\n"
    )]
    #[case::continuation_and_inner_blank(
        TaskId(1),
        "  - [ ] b\n    continuation of b\n\n    - [ ] c\n"
    )]
    #[case::leaf(TaskId(2), "    - [ ] c\n")]
    #[case::paragraph_ends_block(TaskId(4), "- [ ] e\n")]
    #[case::last_line_without_newline(TaskId(5), "- [ ] f")]
    fn block_of_task(#[case] id: TaskId, #[case] expected: &str) {
        let document = TodoDocument::parse(TREE);

        let system_under_test = document.block(id).unwrap();

        assert_eq!(system_under_test.to_string(), expected);
    }

    #[test]
    fn block_lists_its_task_ids() {
        let document = TodoDocument::parse(TREE);
        let expected = vec![TaskId(0), TaskId(1), TaskId(2), TaskId(3)];

        let system_under_test = document.block(TaskId(0)).unwrap();

        assert_eq!(system_under_test.task_ids().collect::<Vec<_>>(), expected);
    }

    #[test]
    fn block_of_unknown_task_is_none() {
        let document = TodoDocument::parse(TREE);

        let system_under_test = document.block(UNKNOWN);

        assert_eq!(system_under_test, None);
    }

    #[rstest]
    #[case::first_task(TaskId(0), Some(2))]
    #[case::after_blank_lines(TaskId(2), Some(6))]
    #[case::last_line(TaskId(5), Some(12))]
    #[case::unknown(UNKNOWN, None)]
    fn line_number_of_task(#[case] id: TaskId, #[case] expected: Option<usize>) {
        let document = TodoDocument::parse(TREE);

        let system_under_test = document.line_number(id);

        assert_eq!(system_under_test, expected);
    }

    #[rstest]
    #[case::no_tasks("# Title\n", spaces(7))]
    #[case::flat("- [ ] a\n- [ ] b\n", spaces(7))]
    #[case::tab("- [ ] a\n\t- [ ] b\n", IndentStyle::Tab)]
    #[case::first_indent_is_tab("- [ ] a\n\t- [ ] b\n- [ ] c\n  - [ ] d\n", IndentStyle::Tab)]
    #[case::two_spaces("- [ ] a\n  - [ ] b\n      - [ ] c\n", spaces(2))]
    #[case::smallest_step("- [ ] a\n    - [ ] b\n- [ ] c\n   - [ ] d\n", spaces(3))]
    #[case::indented_top_level("  - [ ] a\n", spaces(2))]
    #[case::too_wide("- [ ] a\n          - [ ] b\n", spaces(7))]
    #[case::indented_raw_lines_ignored("- [ ] a\n  text\n", spaces(7))]
    fn detects_indent_style(#[case] text: &str, #[case] expected: IndentStyle) {
        let document = TodoDocument::parse(text);

        let system_under_test = document.indent_style(spaces(7));

        assert_eq!(system_under_test, expected);
    }

    /// Fragments that, joined in any order, produce the syntax the parser must handle.
    fn fragment() -> impl Strategy<Value = String> {
        prop_oneof![
            any::<String>(),
            prop::sample::select(vec![
                "\u{feff}",
                "---",
                "...",
                "```",
                "````",
                "~~~",
                "- [ ] a",
                "\t- [x] b",
                "  * [/] c",
                "> - [ ] d",
                "\r\n",
                "\n",
                "\r",
                " ",
                "\t",
            ])
            .prop_map(str::to_owned),
        ]
    }

    proptest! {
        #[test]
        fn arbitrary_text_round_trips(
            text in prop::collection::vec(fragment(), 0..24).prop_map(|parts| parts.concat())
        ) {
            let system_under_test = TodoDocument::parse(&text);

            prop_assert_eq!(system_under_test.to_string(), text);
        }
    }
}

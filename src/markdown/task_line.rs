//! One Markdown task line: parsing and rendering.
//!
//! This module is the single place that knows the syntax of a task (see "Task syntax" in the
//! README). A task line is
//!
//! ```text
//! {indent}{marker}{whitespace}[{status}]{whitespace or end of line}{description}
//! ```
//!
//! where the marker is `-`, `*`, `+` or an ordered marker such as `1.` or `1)`, and the status
//! is exactly one character. Dataview inline fields `[key:: value]` anywhere in the
//! description are extracted into [`InlineField`]s.
//!
//! A [`TaskLine`] remembers its source line until it is modified, so an untouched task renders
//! byte for byte as it was read. A modified task renders canonically as
//! `{indent}{marker} [{status}] {description}` followed by ` [key:: value]` for every field.

use std::fmt::{self, Display, Formatter};
use std::iter;
use std::str::FromStr;

use strum::IntoEnumIterator;

/// Number of columns between two tab stops when measuring indentation.
const TAB_WIDTH: usize = 4;

/// Returns the width in columns of the leading spaces and tabs of `text`. A tab advances to the
/// next multiple of [`TAB_WIDTH`] columns. Task lines and raw lines are measured with this one
/// rule so that their indentations compare.
pub(crate) fn indentation_width(text: &str) -> usize {
    text.chars()
        .take_while(|character| matches!(character, ' ' | '\t'))
        .fold(0, |width, character| {
            if character == '\t' {
                (width / TAB_WIDTH + 1) * TAB_WIDTH
            } else {
                width + 1
            }
        })
}

/// Maximum number of digits of an ordered list marker, as in `CommonMark`.
const MAX_ORDERED_DIGITS: usize = 9;

/// Largest number of an ordered list marker: the largest with [`MAX_ORDERED_DIGITS`] digits.
const MAX_ORDERED_NUMBER: u32 = 999_999_999;

/// Characters that separate the parts of a task line and make up its indentation.
const BLANKS: [char; 2] = [' ', '\t'];

/// Separates the key from the value of an inline field.
const FIELD_SEPARATOR: &str = "::";

/// Errors that occur while constructing an [`InlineField`].
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum InlineFieldError {
    /// The key does not match `[A-Za-z][A-Za-z0-9_-]*`, or it is a known key wrapped in
    /// [`FieldKey::Other`].
    #[error("invalid inline field key `{0}`")]
    InvalidKey(String),
    /// The value contains a backtick, a line break or unbalanced square brackets, so it cannot
    /// be written inside `[key:: value]`.
    #[error("invalid inline field value `{0}`")]
    InvalidValue(String),
}

/// The character after the number of an ordered list marker.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, strum::EnumIter)]
pub enum OrderedDelimiter {
    /// `1.`
    #[default]
    Period,
    /// `1)`
    Parenthesis,
}

impl OrderedDelimiter {
    /// Returns the delimiter character.
    #[must_use]
    pub const fn as_char(self) -> char {
        match self {
            Self::Period => '.',
            Self::Parenthesis => ')',
        }
    }

    /// Splits a delimiter off the start of `text`.
    fn split_prefix(text: &str) -> Option<(Self, &str)> {
        Self::iter().find_map(|delimiter| {
            text.strip_prefix(delimiter.as_char())
                .map(|rest| (delimiter, rest))
        })
    }
}

/// The list marker of a task line.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, strum::EnumIter)]
pub enum ListMarker {
    /// `-`
    Dash,
    /// `*`
    Asterisk,
    /// `+`
    Plus,
    /// An ordered marker such as `3.` or `3)`. The number is kept as written and never
    /// renumbered.
    Ordered {
        /// The number before the delimiter.
        number: u32,
        /// The character after the number.
        delimiter: OrderedDelimiter,
    },
}

impl ListMarker {
    /// Returns the marker for the next item of the same list: an ordered marker with the
    /// following number (kept at the largest valid number), and the same bullet otherwise.
    pub(crate) fn successor(self) -> Self {
        match self {
            Self::Ordered { number, delimiter } => Self::Ordered {
                number: number
                    .checked_add(1)
                    .filter(|&next| next <= MAX_ORDERED_NUMBER)
                    .unwrap_or(number),
                delimiter,
            },
            Self::Dash | Self::Asterisk | Self::Plus => self,
        }
    }

    /// Splits a list marker off the start of `text`.
    fn split_prefix(text: &str) -> Option<(Self, &str)> {
        let mut characters = text.chars();
        let bullet = match characters.next()? {
            '-' => Self::Dash,
            '*' => Self::Asterisk,
            '+' => Self::Plus,
            _ => return Self::split_ordered_prefix(text),
        };
        Some((bullet, characters.as_str()))
    }

    /// Splits an ordered list marker (1 to 9 digits and a delimiter) off the start of `text`.
    fn split_ordered_prefix(text: &str) -> Option<(Self, &str)> {
        let digit_count = text.len() - text.trim_start_matches(|c: char| c.is_ascii_digit()).len();
        if !(1..=MAX_ORDERED_DIGITS).contains(&digit_count) {
            return None;
        }
        let (digits, rest) = text.split_at(digit_count);
        let (delimiter, rest) = OrderedDelimiter::split_prefix(rest)?;
        let number = digits.parse().ok()?;
        Some((Self::Ordered { number, delimiter }, rest))
    }
}

impl Display for ListMarker {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::Dash => formatter.write_str("-"),
            Self::Asterisk => formatter.write_str("*"),
            Self::Plus => formatter.write_str("+"),
            Self::Ordered { number, delimiter } => {
                write!(formatter, "{number}{}", delimiter.as_char())
            }
        }
    }
}

/// The status of a task, written as the character between the square brackets.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, strum::EnumIter)]
pub enum TaskStatus {
    /// `[ ]`
    Todo,
    /// `[x]` or `[X]`; rendered as `[x]`.
    Done,
    /// `[/]`
    InProgress,
    /// `[-]`
    Cancelled,
    /// Any other character, kept so that custom statuses survive an edit.
    Other(char),
}

impl TaskStatus {
    /// Returns the character written between the square brackets.
    #[must_use]
    pub const fn as_char(self) -> char {
        match self {
            Self::Todo => ' ',
            Self::Done => 'x',
            Self::InProgress => '/',
            Self::Cancelled => '-',
            Self::Other(character) => character,
        }
    }

    /// Tells whether nothing is left to do: the task is done or cancelled.
    #[must_use]
    pub const fn is_completed(self) -> bool {
        match self {
            Self::Done | Self::Cancelled => true,
            Self::Todo | Self::InProgress | Self::Other(_) => false,
        }
    }
}

impl From<char> for TaskStatus {
    fn from(character: char) -> Self {
        match character {
            ' ' => Self::Todo,
            'x' | 'X' => Self::Done,
            '/' => Self::InProgress,
            '-' => Self::Cancelled,
            other => Self::Other(other),
        }
    }
}

/// The key of a Dataview inline field.
///
/// Keys RuDo interprets are variants of their own; every other key is kept as
/// [`FieldKey::Other`]. Keys are case sensitive, so `Due` is an other key.
#[derive(Debug, Clone, PartialEq, Eq, Hash, strum::EnumIter)]
pub enum FieldKey {
    /// `due`: the due date.
    Due,
    /// `priority`: the priority.
    Priority,
    /// `repeat`: the recurrence rule.
    Repeat,
    /// `completion`: the date the task was completed.
    Completion,
    /// Any other key matching `[A-Za-z][A-Za-z0-9_-]*`.
    Other(String),
}

impl FieldKey {
    /// Returns the key as written in Markdown.
    #[must_use]
    pub fn as_str(&self) -> &str {
        match self {
            Self::Due => "due",
            Self::Priority => "priority",
            Self::Repeat => "repeat",
            Self::Completion => "completion",
            Self::Other(name) => name,
        }
    }
}

impl FromStr for FieldKey {
    type Err = InlineFieldError;

    /// Parses a key, mapping the names of known keys to their variants.
    ///
    /// # Errors
    ///
    /// Returns [`InlineFieldError::InvalidKey`] if `name` does not match
    /// `[A-Za-z][A-Za-z0-9_-]*`.
    fn from_str(name: &str) -> Result<Self, Self::Err> {
        if name.is_empty() || field_key_len(name) != name.len() {
            return Err(InlineFieldError::InvalidKey(name.to_owned()));
        }
        // `iter` yields `Other` with an empty name, which never equals a valid key, so only
        // known keys are found here and new known keys are picked up automatically.
        Ok(Self::iter()
            .find(|key| key.as_str() == name)
            .unwrap_or_else(|| Self::Other(name.to_owned())))
    }
}

impl Display for FieldKey {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// A Dataview inline field `[key:: value]`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct InlineField {
    key: FieldKey,
    value: String,
}

impl InlineField {
    /// Creates a field from a key and a value. Surrounding whitespace of the value is removed,
    /// as it is when a field is parsed.
    ///
    /// # Errors
    ///
    /// Returns [`InlineFieldError::InvalidKey`] if `key` is a [`FieldKey::Other`] whose name is
    /// not a valid key or is the name of a known key, and [`InlineFieldError::InvalidValue`] if
    /// `value` contains a backtick, a line break or unbalanced square brackets.
    ///
    /// # Examples
    ///
    /// ```
    /// use rudo::markdown::task_line::{FieldKey, InlineField, InlineFieldError};
    ///
    /// let field = InlineField::new(FieldKey::Due, " 2026-12-01 ")?;
    /// assert_eq!(field.to_string(), "[due:: 2026-12-01]");
    /// # Ok::<(), InlineFieldError>(())
    /// ```
    pub fn new(key: FieldKey, value: &str) -> Result<Self, InlineFieldError> {
        if key.as_str().parse::<FieldKey>()? != key {
            return Err(InlineFieldError::InvalidKey(key.as_str().to_owned()));
        }
        let value_with_closing_bracket = value.char_indices().chain(iter::once((value.len(), ']')));
        if closing_bracket_index(value_with_closing_bracket) != Some(value.len()) {
            return Err(InlineFieldError::InvalidValue(value.to_owned()));
        }
        Ok(Self {
            key,
            value: value.trim().to_owned(),
        })
    }

    /// Returns the key.
    #[must_use]
    pub fn key(&self) -> &FieldKey {
        &self.key
    }

    /// Returns the value without surrounding whitespace.
    #[must_use]
    pub fn value(&self) -> &str {
        &self.value
    }

    /// Parses a field at the start of `text` and returns it with its length in bytes.
    fn parse_prefix(text: &str) -> Option<(Self, usize)> {
        let inner = text.strip_prefix('[')?;
        let (name, rest) = inner.split_at(field_key_len(inner));
        let rest = rest.strip_prefix(FIELD_SEPARATOR)?;
        let value_len = closing_bracket_index(rest.char_indices())?;
        let key = name.parse().ok()?;
        let field = Self {
            key,
            value: rest[..value_len].trim().to_owned(),
        };
        let field_len = text.len() - rest.len() + value_len + ']'.len_utf8();
        Some((field, field_len))
    }
}

impl Display for InlineField {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        write!(formatter, "[{}{FIELD_SEPARATOR} {}]", self.key, self.value)
    }
}

/// Returns the byte length of the field key at the start of `text`, which is `0` if `text`
/// does not start with an ASCII letter.
fn field_key_len(text: &str) -> usize {
    if !text.starts_with(|c: char| c.is_ascii_alphabetic()) {
        return 0;
    }
    text.find(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == '-'))
        .unwrap_or(text.len())
}

/// Returns the index of the `]` that closes a field value, skipping balanced pairs of square
/// brackets. Returns `None` if the value is not closed or contains a backtick or a line
/// break, which keeps code spans and lines intact around fields.
fn closing_bracket_index(characters: impl Iterator<Item = (usize, char)>) -> Option<usize> {
    let mut depth = 0_usize;
    for (index, character) in characters {
        match character {
            '`' | '\n' | '\r' => return None,
            '[' => depth += 1,
            ']' if depth == 0 => return Some(index),
            ']' => depth -= 1,
            _ => {}
        }
    }
    None
}

/// Returns the byte length of the code span at the start of `text`, which starts with a
/// backtick. An unclosed backtick run is literal text and only the run itself is returned.
fn code_span_len(text: &str) -> usize {
    let opening_len = backtick_run_len(text);
    let mut search_start = opening_len;
    while let Some(offset) = text[search_start..].find('`') {
        let run_start = search_start + offset;
        let run_len = backtick_run_len(&text[run_start..]);
        if run_len == opening_len {
            return run_start + run_len;
        }
        search_start = run_start + run_len;
    }
    opening_len
}

/// Returns the number of backticks at the start of `text`.
fn backtick_run_len(text: &str) -> usize {
    text.len() - text.trim_start_matches('`').len()
}

/// Splits Dataview inline fields `[key:: value]` off a task description.
///
/// Returns the description without the fields and the fields in their original order. Each
/// field and the whitespace around it become a single space between the surrounding text, and
/// the whitespace around the description is removed. Text in code spans,
/// wikilinks (`[[…]]`), Markdown links, `[x]` and keys with spaces stay in the description, and
/// so does Dataview's hidden-key syntax `(key:: value)`. `text` is expected to be a single
/// line.
///
/// # Examples
///
/// ```
/// use rudo::markdown::task_line::{FieldKey, split_inline_fields};
///
/// let (description, fields) = split_inline_fields("Buy milk [due:: 2026-12-01] #home");
/// assert_eq!(description, "Buy milk #home");
/// assert_eq!(fields[0].key(), &FieldKey::Due);
/// assert_eq!(fields[0].value(), "2026-12-01");
/// ```
#[must_use]
pub fn split_inline_fields(text: &str) -> (String, Vec<InlineField>) {
    let mut description = String::with_capacity(text.len());
    let mut fields = Vec::new();
    let mut rest = text.trim_start();
    let mut previous = None;
    while let Some(character) = rest.chars().next() {
        let text_len = match character {
            '`' => code_span_len(rest),
            // A `[` right after another `[` belongs to a wikilink and cannot start a field.
            '[' if previous != Some('[') => match InlineField::parse_prefix(rest) {
                Some((field, field_len)) => {
                    fields.push(field);
                    rest = remove_field_text(&mut description, &rest[field_len..]);
                    previous = description.chars().next_back();
                    continue;
                }
                None => character.len_utf8(),
            },
            _ => character.len_utf8(),
        };
        let (copied, remaining) = rest.split_at(text_len);
        description.push_str(copied);
        previous = copied.chars().next_back();
        rest = remaining;
    }
    description.truncate(description.trim_end().len());
    (description, fields)
}

/// Replaces a field that was cut out of a description, together with the whitespace around
/// it, by a single space between the text before and after it, and returns the text after it.
///
/// The space keeps the two sides apart, so that removing a field never joins them into new
/// syntax such as one longer backtick run.
fn remove_field_text<'text>(description: &mut String, after_field: &'text str) -> &'text str {
    description.truncate(description.trim_end().len());
    let after_field = after_field.trim_start();
    if !(description.is_empty() || after_field.is_empty()) {
        description.push(' ');
    }
    after_field
}

/// One task line of a Markdown file.
///
/// A task parsed from a line remembers that line and renders it unchanged until one of the
/// mutators is called; from then on it renders canonically.
///
/// # Examples
///
/// ```
/// use rudo::markdown::task_line::{TaskLine, TaskStatus};
///
/// let mut task = TaskLine::parse("-  [ ] Buy [due:: 2026-12-01] milk ").expect("a task line");
/// assert_eq!(task.to_string(), "-  [ ] Buy [due:: 2026-12-01] milk ");
///
/// task.set_status(TaskStatus::Done);
/// assert_eq!(task.to_string(), "- [x] Buy milk [due:: 2026-12-01]");
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct TaskLine {
    indent: String,
    marker: ListMarker,
    status: TaskStatus,
    description: String,
    fields: Vec<InlineField>,
    /// The source line, until the task is modified.
    original: Option<String>,
}

impl TaskLine {
    /// Parses one line of Markdown, without its line ending, as a task.
    ///
    /// Returns `None` if the line is not a task: a list marker (`-`, `*`, `+`, `1.` or `1)`)
    /// after optional indentation, whitespace, `[` and one status character, `]`, and then
    /// whitespace or the end of the line.
    #[must_use]
    pub fn parse(line: &str) -> Option<Self> {
        let content = line.trim_start_matches(BLANKS);
        let indent = &line[..line.len() - content.len()];
        let (marker, rest) = ListMarker::split_prefix(content)?;
        let rest = rest.strip_prefix(BLANKS)?.trim_start_matches(BLANKS);
        let (status, rest) = split_checkbox(rest)?;
        if !(rest.is_empty() || rest.starts_with(BLANKS)) {
            return None;
        }
        let (description, fields) = split_inline_fields(rest);
        Some(Self {
            indent: indent.to_owned(),
            marker,
            status,
            description,
            fields,
            original: Some(line.to_owned()),
        })
    }

    /// Returns the indentation as written: spaces and tabs.
    #[must_use]
    pub fn indent(&self) -> &str {
        &self.indent
    }

    /// Returns the width of the indentation in columns. A tab advances to the next multiple of
    /// four columns.
    #[must_use]
    pub fn indent_width(&self) -> usize {
        indentation_width(&self.indent)
    }

    /// Returns the list marker.
    #[must_use]
    pub fn marker(&self) -> ListMarker {
        self.marker
    }

    /// Returns the status.
    #[must_use]
    pub fn status(&self) -> TaskStatus {
        self.status
    }

    /// Returns the description without inline fields. Tags are part of the description.
    #[must_use]
    pub fn description(&self) -> &str {
        &self.description
    }

    /// Returns the inline fields in their order.
    #[must_use]
    pub fn fields(&self) -> &[InlineField] {
        &self.fields
    }

    /// Returns the first field with the given key.
    #[must_use]
    pub fn field(&self, key: &FieldKey) -> Option<&InlineField> {
        self.fields.iter().find(|field| field.key() == key)
    }

    /// Returns whether the task was changed since it was parsed, in which case it renders
    /// canonically instead of as its source line.
    #[must_use]
    pub fn is_modified(&self) -> bool {
        self.original.is_none()
    }

    /// Sets the status and marks the task as modified.
    pub fn set_status(&mut self, status: TaskStatus) {
        self.status = status;
        self.mark_modified();
    }

    /// Sets the description from text a user typed and marks the task as modified.
    ///
    /// Inline fields in `text` are extracted with [`split_inline_fields`] and set with
    /// [`TaskLine::set_field`], so typing `Buy milk [due:: 2026-12-01]` sets the due date.
    /// Existing fields with other keys are kept.
    pub fn set_description(&mut self, text: &str) {
        let (description, fields) = split_inline_fields(text);
        self.description = description;
        for field in fields {
            self.set_field(field);
        }
        self.mark_modified();
    }

    /// Replaces the first field with the same key and removes the others with that key, or
    /// appends the field if there is none, and marks the task as modified.
    pub fn set_field(&mut self, field: InlineField) {
        match self
            .fields
            .iter()
            .position(|existing| existing.key == field.key)
        {
            Some(first) => {
                let mut index = 0;
                self.fields.retain(|existing| {
                    let keep = index <= first || existing.key != field.key;
                    index += 1;
                    keep
                });
                self.fields[first] = field;
            }
            None => self.fields.push(field),
        }
        self.mark_modified();
    }

    /// Removes the first field with the given key and returns it. The task is marked as
    /// modified only if a field was removed.
    pub fn remove_field(&mut self, key: &FieldKey) -> Option<InlineField> {
        let index = self.fields.iter().position(|field| field.key() == key)?;
        self.mark_modified();
        Some(self.fields.remove(index))
    }

    /// Replaces the indentation, which must consist of spaces and tabs only. An unmodified task
    /// keeps rendering its source line, with the new indentation.
    pub(crate) fn set_indent(&mut self, indent: String) {
        debug_assert!(
            indent.chars().all(|character| BLANKS.contains(&character)),
            "indentation consists of blanks"
        );
        if let Some(original) = &mut self.original {
            original.replace_range(..self.indent.len(), &indent);
        }
        self.indent = indent;
    }

    fn mark_modified(&mut self) {
        self.original = None;
    }
}

/// Splits a checkbox `[c]` with exactly one status character off the start of `text`.
fn split_checkbox(text: &str) -> Option<(TaskStatus, &str)> {
    let mut characters = text.strip_prefix('[')?.chars();
    let status = TaskStatus::from(characters.next()?);
    let rest = characters.as_str().strip_prefix(']')?;
    Some((status, rest))
}

impl Display for TaskLine {
    /// Writes the source line of an unmodified task, and the canonical form of a modified one.
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        if let Some(original) = &self.original {
            return formatter.write_str(original);
        }
        write!(
            formatter,
            "{}{} [{}]",
            self.indent,
            self.marker,
            self.status.as_char()
        )?;
        if !self.description.is_empty() {
            write!(formatter, " {}", self.description)?;
        }
        self.fields
            .iter()
            .try_for_each(|field| write!(formatter, " {field}"))
    }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;
    use rstest::rstest;
    use strum::IntoEnumIterator;

    use super::*;

    /// The parts of a task that a rendering preserves, i.e. everything except whether the task
    /// was modified.
    type TaskParts<'task> = (
        &'task str,
        ListMarker,
        TaskStatus,
        &'task str,
        &'task [InlineField],
    );

    fn parts(task: &TaskLine) -> TaskParts<'_> {
        (
            task.indent(),
            task.marker(),
            task.status(),
            task.description(),
            task.fields(),
        )
    }

    fn field(key: &str, value: &str) -> InlineField {
        InlineField::new(key.parse().unwrap(), value).unwrap()
    }

    fn fields(pairs: &[(&str, &str)]) -> Vec<InlineField> {
        pairs.iter().map(|(key, value)| field(key, value)).collect()
    }

    fn parse_task(line: &str) -> TaskLine {
        TaskLine::parse(line).unwrap()
    }

    const DASH: ListMarker = ListMarker::Dash;

    const fn ordered(number: u32, delimiter: OrderedDelimiter) -> ListMarker {
        ListMarker::Ordered { number, delimiter }
    }

    #[rstest]
    #[case::todo("- [ ] Buy milk", "", DASH, TaskStatus::Todo, "Buy milk")]
    #[case::done_lowercase("- [x] Buy milk", "", DASH, TaskStatus::Done, "Buy milk")]
    #[case::done_uppercase("- [X] Buy milk", "", DASH, TaskStatus::Done, "Buy milk")]
    #[case::in_progress("- [/] Buy milk", "", DASH, TaskStatus::InProgress, "Buy milk")]
    #[case::cancelled("- [-] Buy milk", "", DASH, TaskStatus::Cancelled, "Buy milk")]
    #[case::other_status("- [?] Buy milk", "", DASH, TaskStatus::Other('?'), "Buy milk")]
    #[case::multi_byte_status("- [ü] Milch", "", DASH, TaskStatus::Other('ü'), "Milch")]
    #[case::asterisk("* [ ] a", "", ListMarker::Asterisk, TaskStatus::Todo, "a")]
    #[case::plus("+ [ ] a", "", ListMarker::Plus, TaskStatus::Todo, "a")]
    #[case::ordered_period(
        "1. [ ] a",
        "",
        ordered(1, OrderedDelimiter::Period),
        TaskStatus::Todo,
        "a"
    )]
    #[case::ordered_parenthesis(
        "3) [ ] a",
        "",
        ordered(3, OrderedDelimiter::Parenthesis),
        TaskStatus::Todo,
        "a"
    )]
    #[case::ordered_nine_digits(
        "123456789. [ ] a",
        "",
        ordered(123_456_789, OrderedDelimiter::Period),
        TaskStatus::Todo,
        "a"
    )]
    #[case::empty_description("- [ ]", "", DASH, TaskStatus::Todo, "")]
    #[case::empty_description_trailing_space("- [ ]   ", "", DASH, TaskStatus::Todo, "")]
    #[case::space_indent("    - [ ] a", "    ", DASH, TaskStatus::Todo, "a")]
    #[case::tab_indent("\t\t- [ ] a", "\t\t", DASH, TaskStatus::Todo, "a")]
    #[case::tabs_as_separators("-\t[ ]\ta", "", DASH, TaskStatus::Todo, "a")]
    #[case::several_spaces("-   [ ]   a  b  ", "", DASH, TaskStatus::Todo, "a  b")]
    #[case::tag_stays(
        "- [ ] Call #work/phone",
        "",
        DASH,
        TaskStatus::Todo,
        "Call #work/phone"
    )]
    #[case::umlauts(
        "- [ ] Äpfel für Großmutter",
        "",
        DASH,
        TaskStatus::Todo,
        "Äpfel für Großmutter"
    )]
    #[case::cjk("- [ ] 牛乳を買う", "", DASH, TaskStatus::Todo, "牛乳を買う")]
    #[case::emoji(
        "- [ ] 🥛 Buy milk 📅 2026-12-01",
        "",
        DASH,
        TaskStatus::Todo,
        "🥛 Buy milk 📅 2026-12-01"
    )]
    #[case::bracket_status("- []] a", "", DASH, TaskStatus::Other(']'), "a")]
    fn parse_recognises_task_line(
        #[case] line: &str,
        #[case] expected_indent: &str,
        #[case] expected_marker: ListMarker,
        #[case] expected_status: TaskStatus,
        #[case] expected_description: &str,
    ) {
        let expected_parts: TaskParts<'_> = (
            expected_indent,
            expected_marker,
            expected_status,
            expected_description,
            &[],
        );

        let system_under_test = TaskLine::parse(line);

        assert_eq!(system_under_test.as_ref().map(parts), Some(expected_parts));
    }

    #[rstest]
    #[case::empty("")]
    #[case::plain_text("Buy milk")]
    #[case::heading("# [ ] heading")]
    #[case::plain_list_item("- Buy milk")]
    #[case::no_space_after_marker("-[ ] a")]
    #[case::empty_checkbox("- [] a")]
    #[case::text_after_checkbox("- [ ]text")]
    #[case::two_status_characters("- [xx] a")]
    #[case::unclosed_checkbox("- [ a")]
    #[case::marker_only("-")]
    #[case::marker_and_space("- ")]
    #[case::ordered_without_delimiter("1 [ ] a")]
    #[case::ordered_ten_digits("1234567890. [ ] a")]
    #[case::ordered_other_delimiter("1: [ ] a")]
    #[case::letter_marker("a. [ ] a")]
    #[case::quote("> - [ ] a")]
    #[case::non_breaking_space_after_marker("-\u{a0}[ ] a")]
    #[case::multi_byte_after_checkbox("- [ ]ä")]
    fn parse_rejects_non_task_line(#[case] line: &str) {
        let system_under_test = TaskLine::parse(line);

        assert_eq!(system_under_test, None);
    }

    #[rstest]
    #[case::no_fields("Buy milk", "Buy milk", &[])]
    #[case::trailing_field("Buy milk [due:: 2026-12-01]", "Buy milk", &[("due", "2026-12-01")])]
    #[case::field_in_the_middle("Buy [due:: 2026-12-01] milk", "Buy milk", &[("due", "2026-12-01")])]
    #[case::leading_field("[due:: 2026-12-01] Buy milk", "Buy milk", &[("due", "2026-12-01")])]
    #[case::only_field("[due:: 2026-12-01]", "", &[("due", "2026-12-01")])]
    #[case::fields_keep_order(
        "a [repeat:: every week] [due:: 2026-12-01] [x-y_z:: 1]",
        "a",
        &[("repeat", "every week"), ("due", "2026-12-01"), ("x-y_z", "1")]
    )]
    #[case::duplicate_keys("a [due:: 1] [due:: 2]", "a", &[("due", "1"), ("due", "2")])]
    #[case::adjacent_fields("a [due:: 1][priority:: high]", "a", &[("due", "1"), ("priority", "high")])]
    #[case::field_between_words("a[due:: 1]b", "a b", &[("due", "1")])]
    #[case::field_between_backticks("`a`[due:: 1]`b`", "`a` `b`", &[("due", "1")])]
    #[case::many_spaces_around_field("a   [due:: 1]   b", "a b", &[("due", "1")])]
    #[case::no_space_after_separator("a [due::1]", "a", &[("due", "1")])]
    #[case::value_is_trimmed("a [due::   1  ]", "a", &[("due", "1")])]
    #[case::empty_value("a [due::]", "a", &[("due", "")])]
    #[case::nested_brackets_in_value("a [link:: [[Page]]]", "a", &[("link", "[[Page]]")])]
    #[case::multi_byte_value("a [ort:: Küche 台所]", "a", &[("ort", "Küche 台所")])]
    #[case::tags_stay("a #home [due:: 1] #errand", "a #home #errand", &[("due", "1")])]
    #[case::wikilink("see [[due:: x]]", "see [[due:: x]]", &[])]
    #[case::markdown_link("see [text](https://example.com)", "see [text](https://example.com)", &[])]
    #[case::checkbox_in_text("see [x] here", "see [x] here", &[])]
    #[case::key_with_space("a [due date:: 1]", "a [due date:: 1]", &[])]
    #[case::key_starting_with_digit("a [1due:: 1]", "a [1due:: 1]", &[])]
    #[case::space_before_separator("a [due :: 1]", "a [due :: 1]", &[])]
    #[case::single_colon("a [due: 1]", "a [due: 1]", &[])]
    #[case::unclosed_field("a [due:: 1", "a [due:: 1", &[])]
    #[case::unbalanced_value("a [due:: [1]", "a [due:: [1]", &[])]
    #[case::hidden_key_syntax("a (due:: 1)", "a (due:: 1)", &[])]
    #[case::code_span("a `[due:: 1]` b", "a `[due:: 1]` b", &[])]
    #[case::double_backtick_code_span("a ``x ` [due:: 1]`` b", "a ``x ` [due:: 1]`` b", &[])]
    #[case::unclosed_code_span("a `b [due:: 1]", "a `b", &[("due", "1")])]
    #[case::field_after_code_span("`a` [due:: 1]", "`a`", &[("due", "1")])]
    #[case::backtick_in_value("a [due:: `1`]", "a [due:: `1`]", &[])]
    #[case::uppercase_known_key("a [Due:: 1]", "a", &[("Due", "1")])]
    #[case::surrounding_whitespace("  a b  ", "a b", &[])]
    fn split_inline_fields_extracts_fields(
        #[case] text: &str,
        #[case] expected_description: &str,
        #[case] expected_fields: &[(&str, &str)],
    ) {
        let expected = (expected_description.to_owned(), fields(expected_fields));

        let system_under_test = split_inline_fields(text);

        assert_eq!(system_under_test, expected);
    }

    #[test]
    fn parse_extracts_inline_fields() {
        let line = "  - [ ] Buy [due:: 2026-12-01] milk #home [priority:: high]";
        let expected_fields = fields(&[("due", "2026-12-01"), ("priority", "high")]);
        let expected_parts: TaskParts<'_> = (
            "  ",
            DASH,
            TaskStatus::Todo,
            "Buy milk #home",
            &expected_fields,
        );

        let system_under_test = parse_task(line);

        assert_eq!(parts(&system_under_test), expected_parts);
    }

    #[rstest]
    #[case::due("due", FieldKey::Due)]
    #[case::priority("priority", FieldKey::Priority)]
    #[case::repeat("repeat", FieldKey::Repeat)]
    #[case::completion("completion", FieldKey::Completion)]
    #[case::other("estimate", FieldKey::Other("estimate".to_owned()))]
    #[case::other_uppercase_known("Due", FieldKey::Other("Due".to_owned()))]
    #[case::digits_dash_underscore("a1-_b", FieldKey::Other("a1-_b".to_owned()))]
    fn field_key_parses_valid_name(#[case] name: &str, #[case] expected: FieldKey) {
        let system_under_test = name.parse::<FieldKey>();

        assert_eq!(system_under_test, Ok(expected));
    }

    #[rstest]
    #[case::empty("")]
    #[case::leading_digit("1a")]
    #[case::leading_dash("-a")]
    #[case::space("due date")]
    #[case::colon("a:b")]
    #[case::umlaut("fällig")]
    fn field_key_rejects_invalid_name(#[case] name: &str) {
        let expected = Err(InlineFieldError::InvalidKey(name.to_owned()));

        let system_under_test = name.parse::<FieldKey>();

        assert_eq!(system_under_test, expected);
    }

    #[test]
    fn field_key_name_round_trips_for_every_variant() {
        let keys: Vec<FieldKey> = FieldKey::iter()
            .map(|key| match key {
                FieldKey::Other(_) => FieldKey::Other("estimate".to_owned()),
                known => known,
            })
            .collect();

        let system_under_test: Vec<Result<FieldKey, InlineFieldError>> =
            keys.iter().map(|key| key.as_str().parse()).collect();

        assert_eq!(
            system_under_test,
            keys.into_iter().map(Ok).collect::<Vec<_>>()
        );
    }

    #[rstest]
    #[case::trims_value(FieldKey::Due, "  2026-12-01 ", "[due:: 2026-12-01]")]
    #[case::empty_value(FieldKey::Due, "", "[due:: ]")]
    #[case::nested_brackets(FieldKey::Other("link".to_owned()), "[[Page]]", "[link:: [[Page]]]")]
    #[case::multi_byte(FieldKey::Other("ort".to_owned()), "Küche", "[ort:: Küche]")]
    fn inline_field_renders_as_dataview_field(
        #[case] key: FieldKey,
        #[case] value: &str,
        #[case] expected: &str,
    ) {
        let system_under_test = InlineField::new(key, value).map(|field| field.to_string());

        assert_eq!(system_under_test.as_deref(), Ok(expected));
    }

    #[rstest]
    #[case::invalid_other_key(FieldKey::Other("due date".to_owned()), "1")]
    #[case::known_key_as_other(FieldKey::Other("due".to_owned()), "1")]
    fn inline_field_rejects_invalid_key(#[case] key: FieldKey, #[case] value: &str) {
        let expected = Err(InlineFieldError::InvalidKey(key.as_str().to_owned()));

        let system_under_test = InlineField::new(key, value);

        assert_eq!(system_under_test, expected);
    }

    #[rstest]
    #[case::closing_bracket("a]b")]
    #[case::unclosed_bracket("[a")]
    #[case::backtick("`a`")]
    #[case::line_feed("a\nb")]
    #[case::carriage_return("a\rb")]
    fn inline_field_rejects_invalid_value(#[case] value: &str) {
        let expected = Err(InlineFieldError::InvalidValue(value.to_owned()));

        let system_under_test = InlineField::new(FieldKey::Due, value);

        assert_eq!(system_under_test, expected);
    }

    #[rstest]
    #[case::space(' ', TaskStatus::Todo)]
    #[case::lowercase_x('x', TaskStatus::Done)]
    #[case::uppercase_x('X', TaskStatus::Done)]
    #[case::slash('/', TaskStatus::InProgress)]
    #[case::dash('-', TaskStatus::Cancelled)]
    #[case::question_mark('?', TaskStatus::Other('?'))]
    #[case::multi_byte('✓', TaskStatus::Other('✓'))]
    fn task_status_from_character(#[case] character: char, #[case] expected: TaskStatus) {
        let system_under_test = TaskStatus::from(character);

        assert_eq!(system_under_test, expected);
    }

    #[test]
    fn task_status_character_round_trips_for_every_variant() {
        let statuses: Vec<TaskStatus> = TaskStatus::iter().collect();

        let system_under_test: Vec<TaskStatus> = statuses
            .iter()
            .map(|status| TaskStatus::from(status.as_char()))
            .collect();

        assert_eq!(system_under_test, statuses);
    }

    #[test]
    fn list_marker_rendering_parses_back_for_every_variant() {
        let markers: Vec<ListMarker> = ListMarker::iter()
            .chain(OrderedDelimiter::iter().map(|delimiter| ordered(42, delimiter)))
            .collect();

        let system_under_test: Vec<ListMarker> = markers
            .iter()
            .map(|marker| parse_task(&format!("{marker} [ ]")).marker())
            .collect();

        assert_eq!(system_under_test, markers);
    }

    #[rstest]
    #[case::none("- [ ] a", 0)]
    #[case::spaces("   - [ ] a", 3)]
    #[case::tab("\t- [ ] a", 4)]
    #[case::two_tabs("\t\t- [ ] a", 8)]
    #[case::space_then_tab("  \t- [ ] a", 4)]
    #[case::four_spaces_then_tab("    \t- [ ] a", 8)]
    #[case::tab_then_space("\t - [ ] a", 5)]
    fn indent_width_counts_tab_to_next_multiple_of_four(
        #[case] line: &str,
        #[case] expected: usize,
    ) {
        let task = parse_task(line);

        let system_under_test = task.indent_width();

        assert_eq!(system_under_test, expected);
    }

    #[rstest]
    #[case::empty("", 0)]
    #[case::no_indentation("Some prose", 0)]
    #[case::spaces_before_text("  continued note", 2)]
    #[case::tab_then_space_before_text("\t text", 5)]
    #[case::blank_line_of_spaces("   ", 3)]
    #[case::stops_at_first_non_blank("  a\t", 2)]
    fn indentation_width_measures_only_leading_blanks(#[case] text: &str, #[case] expected: usize) {
        let system_under_test = indentation_width(text);

        assert_eq!(system_under_test, expected);
    }

    #[rstest]
    #[case::trailing_whitespace("- [ ] Buy milk   ")]
    #[case::uppercase_done("- [X] a")]
    #[case::field_in_the_middle("-  [ ]  Buy [due::1]  milk")]
    #[case::ordered_number_kept("3) [ ] a")]
    #[case::tab_separated("\t*\t[/]\ta")]
    fn unmodified_task_renders_source_line(#[case] line: &str) {
        let task = parse_task(line);

        let system_under_test = task.to_string();

        assert_eq!(system_under_test, line);
    }

    #[rstest]
    #[case::status("- [ ] a", TaskStatus::Done, "- [x] a")]
    #[case::removes_trailing_whitespace("- [ ] a   ", TaskStatus::Todo, "- [ ] a")]
    #[case::moves_fields_to_end(
        "\t-  [ ]  Buy [due::1] milk",
        TaskStatus::InProgress,
        "\t- [/] Buy milk [due:: 1]"
    )]
    #[case::keeps_ordered_number("3) [ ] a", TaskStatus::Cancelled, "3) [-] a")]
    #[case::keeps_other_status("- [ ] a", TaskStatus::Other('?'), "- [?] a")]
    #[case::empty_description("- [ ]   ", TaskStatus::Done, "- [x]")]
    #[case::empty_description_with_field("- [ ] [due:: 1]", TaskStatus::Done, "- [x] [due:: 1]")]
    fn modified_task_renders_canonically(
        #[case] line: &str,
        #[case] status: TaskStatus,
        #[case] expected: &str,
    ) {
        let mut task = parse_task(line);
        task.set_status(status);

        let system_under_test = task.to_string();

        assert_eq!(system_under_test, expected);
    }

    #[test]
    fn parsed_task_is_not_modified() {
        let line = "- [ ] a";

        let system_under_test = parse_task(line);

        assert!(!system_under_test.is_modified());
    }

    #[test]
    fn set_status_changes_status_and_marks_modified() {
        let mut task = parse_task("- [ ] a");
        task.set_status(TaskStatus::Done);

        let system_under_test = task;

        assert_eq!(system_under_test.status(), TaskStatus::Done);
        assert!(system_under_test.is_modified());
    }

    #[test]
    fn set_description_extracts_typed_fields_and_keeps_others() {
        let mut task = parse_task("- [ ] old [due:: 1] [estimate:: 2h]");
        task.set_description("  Buy milk [due:: 2026-12-01] [priority:: high] ");
        let expected_fields = fields(&[
            ("due", "2026-12-01"),
            ("estimate", "2h"),
            ("priority", "high"),
        ]);

        let system_under_test = task;

        assert_eq!(system_under_test.description(), "Buy milk");
        assert_eq!(system_under_test.fields(), expected_fields);
        assert!(system_under_test.is_modified());
        assert_eq!(
            system_under_test.to_string(),
            "- [ ] Buy milk [due:: 2026-12-01] [estimate:: 2h] [priority:: high]"
        );
    }

    #[test]
    fn set_field_replaces_existing_value_in_place() {
        let mut task = parse_task("- [ ] a [due:: 1] [priority:: low]");
        task.set_field(field("due", "2"));
        let expected_fields = fields(&[("due", "2"), ("priority", "low")]);

        let system_under_test = task;

        assert_eq!(system_under_test.fields(), expected_fields);
        assert!(system_under_test.is_modified());
    }

    #[test]
    fn set_field_replaces_first_duplicate_and_removes_the_others() {
        let mut task = parse_task("- [ ] a [due:: 1] [priority:: low] [due:: 2] [due:: 3]");
        task.set_field(field("due", "4"));
        let expected_fields = fields(&[("due", "4"), ("priority", "low")]);

        let system_under_test = task;

        assert_eq!(system_under_test.fields(), expected_fields);
    }

    #[test]
    fn set_field_appends_new_key() {
        let mut task = parse_task("- [ ] a [due:: 1]");
        task.set_field(field("repeat", "every week"));
        let expected_fields = fields(&[("due", "1"), ("repeat", "every week")]);

        let system_under_test = task;

        assert_eq!(system_under_test.fields(), expected_fields);
        assert!(system_under_test.is_modified());
    }

    #[test]
    fn field_finds_first_field_with_key() {
        let task = parse_task("- [ ] a [estimate:: 1] [due:: 2] [due:: 3]");
        let expected = field("due", "2");

        let system_under_test = task.field(&FieldKey::Due);

        assert_eq!(system_under_test, Some(&expected));
    }

    #[test]
    fn remove_field_removes_first_field_with_key_and_marks_modified() {
        let mut task = parse_task("- [ ] a [due:: 1] [priority:: low]");
        let removed = task.remove_field(&FieldKey::Due);
        let expected_removed = Some(field("due", "1"));
        let expected_fields = fields(&[("priority", "low")]);

        let system_under_test = task;

        assert_eq!(removed, expected_removed);
        assert_eq!(system_under_test.fields(), expected_fields);
        assert!(system_under_test.is_modified());
    }

    #[test]
    fn remove_missing_field_keeps_task_unmodified() {
        let mut task = parse_task("- [ ] a [due:: 1]");
        let removed = task.remove_field(&FieldKey::Repeat);

        let system_under_test = task;

        assert_eq!(removed, None);
        assert!(!system_under_test.is_modified());
    }

    #[rstest]
    #[case::dash(ListMarker::Dash, ListMarker::Dash)]
    #[case::asterisk(ListMarker::Asterisk, ListMarker::Asterisk)]
    #[case::plus(ListMarker::Plus, ListMarker::Plus)]
    #[case::ordered(
        ListMarker::Ordered { number: 9, delimiter: OrderedDelimiter::Parenthesis },
        ListMarker::Ordered { number: 10, delimiter: OrderedDelimiter::Parenthesis }
    )]
    #[case::largest_number(
        ListMarker::Ordered { number: MAX_ORDERED_NUMBER, delimiter: OrderedDelimiter::Period },
        ListMarker::Ordered { number: MAX_ORDERED_NUMBER, delimiter: OrderedDelimiter::Period }
    )]
    fn successor_marker(#[case] marker: ListMarker, #[case] expected: ListMarker) {
        let system_under_test = marker.successor();

        assert_eq!(system_under_test, expected);
    }

    #[test]
    fn largest_ordered_number_has_maximum_digit_count() {
        let expected = MAX_ORDERED_DIGITS;

        let system_under_test = MAX_ORDERED_NUMBER.to_string().len();

        assert_eq!(system_under_test, expected);
    }

    #[test]
    fn set_indent_keeps_the_source_line_of_an_unmodified_task() {
        let mut task = parse_task("\t-  [ ] a [due:: 1] ");
        let expected = "  -  [ ] a [due:: 1] ";
        task.set_indent("  ".to_owned());

        let system_under_test = task;

        assert_eq!(system_under_test.indent(), "  ");
        assert_eq!(system_under_test.to_string(), expected);
        assert!(!system_under_test.is_modified());
    }

    #[test]
    fn set_indent_of_a_modified_task_renders_canonically() {
        let mut task = parse_task("- [ ]   a");
        let expected = "\t- [x] a";
        task.set_status(TaskStatus::Done);
        task.set_indent("\t".to_owned());

        let system_under_test = task;

        assert_eq!(system_under_test.to_string(), expected);
    }

    /// Text with the syntax the field extraction must handle: fields, code spans and wikilinks.
    const DESCRIPTION_TEXT: &str =
        "([^\r\n]|\\[[a-zA-Z][a-z_-]{0,3}:: ?[^\r\n\\]]{0,5}\\]|`{1,2}|\\[\\[|\\]\\]){0,12}";

    /// The start of a task line up to its checkbox, always valid.
    const TASK_PREFIX: &str = "[ \t]{0,5}([-*+]|[0-9]{1,9}[.)])[ \t]{1,2}\\[[^\r\n]\\]";

    /// The start of a task line up to its checkbox, often with one of the near misses the
    /// parser must reject.
    const TASK_LIKE_PREFIX: &str = "[ \t]{0,5}([-*+]|[0-9]{1,10}[.)])[ \t]{0,2}\\[.{0,2}\\]";

    fn line(prefix: &'static str, separator: &'static str) -> impl Strategy<Value = String> {
        (prefix, separator, DESCRIPTION_TEXT)
            .prop_map(|(prefix, separator, text)| format!("{prefix}{separator}{text}"))
    }

    fn modified_task() -> impl Strategy<Value = TaskLine> {
        (
            line(TASK_PREFIX, "[ \t]"),
            any::<char>().prop_filter("line break", |status| !matches!(status, '\r' | '\n')),
            DESCRIPTION_TEXT,
            proptest::collection::vec(("[a-zA-Z][a-zA-Z0-9_-]{0,5}", "[^\r\n]{0,8}"), 0..3),
        )
            .prop_map(|(line, status, description, new_fields)| {
                let mut task = TaskLine::parse(&line).expect("the line starts with a task prefix");
                task.set_status(TaskStatus::from(status));
                task.set_description(&description);
                new_fields
                    .iter()
                    .filter_map(|(key, value)| InlineField::new(key.parse().ok()?, value).ok())
                    .for_each(|new_field| task.set_field(new_field));
                task
            })
    }

    proptest! {
        #[test]
        fn unmodified_task_renders_as_parsed_string(
            line in prop_oneof![any::<String>(), line(TASK_LIKE_PREFIX, "[ \t]{0,2}")]
        ) {
            let system_under_test = TaskLine::parse(&line).map(|task| task.to_string());

            prop_assert!(system_under_test.is_none_or(|rendered| rendered == line));
        }

        #[test]
        fn modified_task_parses_back_from_its_rendering(task in modified_task()) {
            let system_under_test = TaskLine::parse(&task.to_string());

            prop_assert_eq!(system_under_test.as_ref().map(parts), Some(parts(&task)));
        }
    }

    #[rstest]
    #[case::todo(TaskStatus::Todo, false)]
    #[case::done(TaskStatus::Done, true)]
    #[case::in_progress(TaskStatus::InProgress, false)]
    #[case::cancelled(TaskStatus::Cancelled, true)]
    #[case::other(TaskStatus::Other('?'), false)]
    fn only_done_and_cancelled_are_completed(#[case] status: TaskStatus, #[case] expected: bool) {
        let system_under_test = status.is_completed();

        assert_eq!(system_under_test, expected);
    }
}

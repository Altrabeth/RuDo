//! Query terms of the search filter (`/`) and go to anything (`ctrl-p`): words such as
//! `due:today`, `created:>-1m`, `priority:high`, `#tag` or `is:open` that match tasks by their
//! data, mixed with plain text words.
//!
//! [`Query::parse`] turns the typed text into a typed [`Query`] once per keystroke, relative to
//! today, so that matching many tasks parses nothing again. A word with a known prefix is a
//! term; every other word is text, also a word with an unknown prefix such as `foo:bar`, so
//! that existing searches keep working. A term with an invalid value makes the whole query
//! invalid. Every word must match. Dates are written as in the due date prompt
//! ([`parse_due`]), and [`QUERY_TERMS`] describes the syntax for the help screen.

use std::cmp::Ordering;
use std::fmt;
use std::sync::LazyLock;

use jiff::civil::Date;
use strum::IntoEnumIterator;

use crate::app::filter::{SmartCaseWords, matched_length};
use crate::domain::date::{DateExprError, due_keywords, parse_due};
use crate::domain::priority::{self, Priority};
use crate::domain::tags;
use crate::markdown::task_line::{FieldKey, TaskLine, TaskStatus};

/// Separates the prefix of a term from its value.
const TERM_SEPARATOR: char = ':';
/// Starts a tag.
const TAG_HASH: &str = "#";
/// Separates the levels of a nested tag.
const TAG_LEVEL_SEPARATOR: char = '/';
/// Date term value: tasks without the date.
const NO_DATE: &str = "none";
/// Date term value: tasks with the date.
const ANY_DATE: &str = "any";
/// Due date term value: open tasks due before today.
const OVERDUE: &str = "overdue";

/// The data of a task that a [`Query`] matches.
pub trait TaskData {
    /// Returns the description without inline fields, with its tags.
    fn description(&self) -> &str;

    /// Returns the status.
    fn status(&self) -> TaskStatus;

    /// Returns the date `field`, or `None` if the task has none.
    fn date(&self, field: DateField) -> Option<Date>;

    /// Returns the priority, `None` for normal priority.
    fn priority(&self) -> Option<Priority>;
}

impl TaskData for TaskLine {
    fn description(&self) -> &str {
        TaskLine::description(self)
    }

    fn status(&self) -> TaskStatus {
        TaskLine::status(self)
    }

    fn date(&self, field: DateField) -> Option<Date> {
        self.field_date(&field.key())
    }

    fn priority(&self) -> Option<Priority> {
        TaskLine::priority(self)
    }
}

/// The data of a task that a [`Query`] matches, copied out of its [`TaskLine`] once, so that
/// it can be matched while the document is not at hand, as in go to anything.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct TaskSnapshot {
    description: String,
    status: TaskStatus,
    due: Option<Date>,
    created: Option<Date>,
    done: Option<Date>,
    priority: Option<Priority>,
}

impl From<&TaskLine> for TaskSnapshot {
    fn from(task: &TaskLine) -> Self {
        Self {
            description: task.description().to_owned(),
            status: task.status(),
            due: TaskData::date(task, DateField::Due),
            created: TaskData::date(task, DateField::Created),
            done: TaskData::date(task, DateField::Done),
            priority: task.priority(),
        }
    }
}

impl TaskData for TaskSnapshot {
    fn description(&self) -> &str {
        &self.description
    }

    fn status(&self) -> TaskStatus {
        self.status
    }

    fn date(&self, field: DateField) -> Option<Date> {
        match field {
            DateField::Due => self.due,
            DateField::Created => self.created,
            DateField::Done => self.done,
        }
    }

    fn priority(&self) -> Option<Priority> {
        self.priority
    }
}

/// A date of a task that a term compares.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, strum::EnumIter)]
pub enum DateField {
    /// The due date.
    Due,
    /// The creation date.
    Created,
    /// The completion date.
    Done,
}

impl DateField {
    /// Returns the key of the inline field that stores the date.
    #[must_use]
    pub fn key(self) -> FieldKey {
        match self {
            Self::Due => FieldKey::Due,
            Self::Created => FieldKey::Created,
            Self::Done => FieldKey::Completion,
        }
    }

    const fn term_key(self) -> TermKey {
        match self {
            Self::Due => TermKey::Due,
            Self::Created => TermKey::Created,
            Self::Done => TermKey::Done,
        }
    }
}

/// The prefix of a term, written before the `:`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, strum::EnumIter)]
#[non_exhaustive]
pub enum TermKey {
    /// `due:`
    Due,
    /// `created:`
    Created,
    /// `done:`
    Done,
    /// `priority:`
    Priority,
    /// `is:`
    Is,
}

impl TermKey {
    /// Returns the prefix without the `:`, such as `due`.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Due => "due",
            Self::Created => "created",
            Self::Done => "done",
            Self::Priority => "priority",
            Self::Is => "is",
        }
    }

    fn from_prefix(prefix: &str) -> Option<Self> {
        Self::iter().find(|key| key.as_str() == prefix)
    }

    /// Parses the non-empty `value` of a term with this prefix, relative to `today`.
    fn parse_value(self, value: &str, today: Date) -> Result<Term, TermValueError> {
        let date_term = |field: DateField| {
            DateCondition::parse(field, value, today)
                .map(|condition| Term::Date { field, condition })
        };
        match self {
            Self::Due => date_term(DateField::Due),
            Self::Created => date_term(DateField::Created),
            Self::Done => date_term(DateField::Done),
            Self::Priority => parse_priority(value),
            Self::Is => StatusFilter::parse(value).map(Term::Status),
        }
    }
}

impl fmt::Display for TermKey {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// Why the value of a term is invalid.
#[derive(Debug, Clone, thiserror::Error)]
#[non_exhaustive]
pub enum TermValueError {
    /// Nothing follows the prefix or the comparison.
    #[error("missing value")]
    Missing,
    /// The value is no date expression.
    #[error(transparent)]
    Date(#[from] DateExprError),
    /// The value is no priority.
    #[error("unknown priority `{0}`")]
    Priority(String),
    /// The value is no status.
    #[error("unknown status `{0}`")]
    Status(String),
}

/// A term of a query whose value is invalid, such as `due:blah`.
#[derive(Debug, Clone, thiserror::Error)]
#[error("{key}: {reason}")]
pub struct QueryError {
    key: TermKey,
    reason: TermValueError,
}

impl QueryError {
    /// Returns the prefix of the invalid term.
    #[must_use]
    pub const fn key(&self) -> TermKey {
        self.key
    }

    /// Returns why its value is invalid.
    #[must_use]
    pub const fn reason(&self) -> &TermValueError {
        &self.reason
    }
}

/// How a value of a task compares to the value of a term.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Comparison {
    Less,
    LessOrEqual,
    Equal,
    GreaterOrEqual,
    Greater,
}

impl Comparison {
    /// The operators that may start a value, longer ones first so that `<=` is not read as `<`.
    const OPERATORS: [(&str, Self); 4] = [
        ("<=", Self::LessOrEqual),
        (">=", Self::GreaterOrEqual),
        ("<", Self::Less),
        (">", Self::Greater),
    ];

    /// Splits the operator off `value`, which compares for equality without one.
    fn split(value: &str) -> (Self, &str) {
        Self::OPERATORS
            .iter()
            .find_map(|&(operator, comparison)| {
                value.strip_prefix(operator).map(|rest| (comparison, rest))
            })
            .unwrap_or((Self::Equal, value))
    }

    /// Tells whether a value of a task that compares to the term value as `ordering` matches.
    const fn holds(self, ordering: Ordering) -> bool {
        match self {
            Self::Less => ordering.is_lt(),
            Self::LessOrEqual => ordering.is_le(),
            Self::Equal => ordering.is_eq(),
            Self::GreaterOrEqual => ordering.is_ge(),
            Self::Greater => ordering.is_gt(),
        }
    }
}

/// What a date term asks of a date of a task.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum DateCondition {
    /// The task has the date and it compares to this one.
    Compare(Comparison, Date),
    /// The task has no such date.
    Missing,
    /// The task has the date.
    Present,
    /// The task is open and due before this day, today.
    Overdue(Date),
}

impl DateCondition {
    fn parse(field: DateField, value: &str, today: Date) -> Result<Self, TermValueError> {
        if value.eq_ignore_ascii_case(NO_DATE) {
            return Ok(Self::Missing);
        }
        if value.eq_ignore_ascii_case(ANY_DATE) {
            return Ok(Self::Present);
        }
        match field {
            DateField::Due if value.eq_ignore_ascii_case(OVERDUE) => {
                return Ok(Self::Overdue(today));
            }
            DateField::Due | DateField::Created | DateField::Done => {}
        }
        let (comparison, expression) = Comparison::split(value);
        let date = parse_due(expression, today)?.ok_or(TermValueError::Missing)?;
        Ok(Self::Compare(comparison, date))
    }

    fn matches(self, date: Option<Date>, status: TaskStatus) -> bool {
        match self {
            Self::Compare(comparison, wanted) => {
                date.is_some_and(|date| comparison.holds(date.cmp(&wanted)))
            }
            Self::Missing => date.is_none(),
            Self::Present => date.is_some(),
            Self::Overdue(today) => !status.is_completed() && date.is_some_and(|due| due < today),
        }
    }
}

/// The status an `is:` term asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, strum::EnumIter)]
enum StatusFilter {
    /// Not done and not cancelled.
    Open,
    /// Done or cancelled.
    Done,
}

impl StatusFilter {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Open => "open",
            Self::Done => "done",
        }
    }

    fn parse(value: &str) -> Result<Self, TermValueError> {
        Self::iter()
            .find(|status| status.as_str().eq_ignore_ascii_case(value))
            .ok_or_else(|| TermValueError::Status(value.to_owned()))
    }

    const fn matches(self, status: TaskStatus) -> bool {
        match self {
            Self::Open => !status.is_completed(),
            Self::Done => status.is_completed(),
        }
    }
}

fn parse_priority(value: &str) -> Result<Term, TermValueError> {
    let (comparison, name) = Comparison::split(value);
    if name.is_empty() {
        return Err(TermValueError::Missing);
    }
    let priority: Priority = name
        .to_ascii_lowercase()
        .parse()
        .map_err(|_unknown| TermValueError::Priority(name.to_owned()))?;
    Ok(Term::Priority {
        comparison,
        level: priority::level(Some(priority)),
    })
}

/// A word of a query that matches tasks by their data.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum Term {
    Date {
        field: DateField,
        condition: DateCondition,
    },
    /// The priority compares to the level, see [`priority::level`].
    Priority {
        comparison: Comparison,
        level: usize,
    },
    /// The task has the tag, given in lower case without the `#`, or a tag nested in it.
    Tag(String),
    Status(StatusFilter),
}

impl Term {
    /// Parses `word`, which contains no whitespace, as a term relative to `today`; `None` if
    /// it is text.
    fn parse(word: &str, today: Date) -> Result<Option<Self>, QueryError> {
        if let Some(tag) = tag_name(word) {
            return Ok(Some(Self::Tag(
                tag.chars().flat_map(char::to_lowercase).collect(),
            )));
        }
        let Some((key, value)) = word
            .split_once(TERM_SEPARATOR)
            .and_then(|(prefix, value)| Some((TermKey::from_prefix(prefix)?, value)))
        else {
            return Ok(None);
        };
        let parsed = if value.is_empty() {
            Err(TermValueError::Missing)
        } else {
            key.parse_value(value, today)
        };
        parsed
            .map(Some)
            .map_err(|reason| QueryError { key, reason })
    }

    fn matches(&self, task: &impl TaskData) -> bool {
        match self {
            Self::Date { field, condition } => condition.matches(task.date(*field), task.status()),
            Self::Priority { comparison, level } => {
                comparison.holds(priority::level(task.priority()).cmp(level))
            }
            Self::Tag(tag) => has_tag(task.description(), tag),
            Self::Status(status) => status.matches(task.status()),
        }
    }
}

/// Returns the name of the tag that `word` consists of, without the `#`, or `None` if the
/// word is not exactly one tag.
fn tag_name(word: &str) -> Option<&str> {
    let span = tags::spans(word).next()?;
    (span == (0..word.len())).then(|| &word[TAG_HASH.len()..])
}

/// Tells whether `description` has the tag `lowercase_tag`, ignoring case, or a tag nested in
/// it, such as `#a/b` for `a`.
fn has_tag(description: &str, lowercase_tag: &str) -> bool {
    tags::spans(description).any(|span| {
        let name = &description[span.start + TAG_HASH.len()..span.end];
        matched_length(name, lowercase_tag).is_some_and(|length| {
            let rest = &name[length..];
            rest.is_empty() || rest.starts_with(TAG_LEVEL_SEPARATOR)
        })
    })
}

/// A parsed search query: its terms and its text words.
///
/// # Examples
///
/// ```
/// use jiff::civil::date;
/// use rudo::app::query::{Query, QueryError};
/// use rudo::markdown::task_line::TaskLine;
///
/// let today = date(2026, 11, 24);
/// let query = Query::parse("milk due:<=fri is:open", today)?;
/// let task = TaskLine::parse("- [ ] Buy milk [due:: 2026-11-26]").expect("a task line");
/// assert!(query.matches(&task));
/// assert_eq!(query.text(), "milk");
/// # Ok::<(), QueryError>(())
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
pub struct Query {
    terms: Vec<Term>,
    /// The typed text with the terms cut out.
    text: String,
    words: SmartCaseWords,
}

impl Query {
    /// Parses `input` relative to `today`: words with a known prefix or a tag are terms, all
    /// other words text.
    ///
    /// # Errors
    ///
    /// Returns a [`QueryError`] for the first term whose value is missing or invalid, such as
    /// `due:` or `priority:urgent`.
    pub fn parse(input: &str, today: Date) -> Result<Self, QueryError> {
        let mut terms = Vec::new();
        let mut text = String::with_capacity(input.len());
        let mut rest = input;
        while !rest.is_empty() {
            let (word, after) = rest.split_at(rest.find(char::is_whitespace).unwrap_or(rest.len()));
            let (gap, next) = after.split_at(
                after
                    .find(|character: char| !character.is_whitespace())
                    .unwrap_or(after.len()),
            );
            rest = next;
            match Term::parse(word, today)? {
                // The whitespace after a term goes with it, or for the last word the one before.
                Some(term) if next.is_empty() => {
                    terms.push(term);
                    text.truncate(text.trim_end().len());
                    text.push_str(gap);
                }
                Some(term) => terms.push(term),
                None => {
                    text.push_str(word);
                    text.push_str(gap);
                }
            }
        }
        let words = SmartCaseWords::new(&text);
        Ok(Self { terms, text, words })
    }

    /// Tells whether the query has neither terms nor text, so that it matches everything.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.terms.is_empty() && self.words.is_empty()
    }

    /// Tells whether the query has at least one term.
    #[must_use]
    pub fn has_terms(&self) -> bool {
        !self.terms.is_empty()
    }

    /// Returns the typed text with the terms and the whitespace of one side of each cut out,
    /// for fuzzy matching; the typed text itself without terms.
    #[must_use]
    pub fn text(&self) -> &str {
        &self.text
    }

    /// Returns the text words, for substring matching and highlighting.
    #[must_use]
    pub fn words(&self) -> &SmartCaseWords {
        &self.words
    }

    /// Tells whether `task` matches every term.
    #[must_use]
    pub fn matches_terms(&self, task: &impl TaskData) -> bool {
        self.terms.iter().all(|term| term.matches(task))
    }

    /// Tells whether `task` matches every term and its description contains every text word.
    #[must_use]
    pub fn matches(&self, task: &impl TaskData) -> bool {
        self.words.matches(task.description()) && self.matches_terms(task)
    }
}

/// Every term with a fixed value, such as `due:today` or `is:open`, for completing a word of
/// the search prompt with `tab`.
pub fn term_words() -> impl Iterator<Item = &'static str> {
    static WORDS: LazyLock<Vec<String>> = LazyLock::new(|| {
        let date_words = DateField::iter().flat_map(|field| {
            let overdue = match field {
                DateField::Due => Some(OVERDUE),
                DateField::Created | DateField::Done => None,
            };
            // `none` is a due keyword already.
            due_keywords()
                .chain(overdue)
                .chain([ANY_DATE])
                .map(move |value| term(field.term_key(), value))
        });
        let priority_words =
            Priority::iter().map(|priority| term(TermKey::Priority, priority.as_str()));
        let status_words = StatusFilter::iter().map(|status| term(TermKey::Is, status.as_str()));
        date_words
            .chain(priority_words)
            .chain(status_words)
            .collect()
    });
    WORDS.iter().map(String::as_str)
}

/// Writes the term with `key` and `value`, such as `due:today`.
fn term(key: TermKey, value: &str) -> String {
    format!("{key}{TERM_SEPARATOR}{value}")
}

/// One row of the syntax of query terms, on the help screen and in `docs/keybindings.md`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TermSyntax {
    examples: &'static [&'static str],
    description: &'static str,
}

impl TermSyntax {
    /// Returns examples of the syntax, such as `due:today`.
    #[must_use]
    pub const fn examples(&self) -> &'static [&'static str] {
        self.examples
    }

    /// Returns what the terms match.
    #[must_use]
    pub const fn description(&self) -> &'static str {
        self.description
    }
}

/// The syntax of query terms, in the order of the help screen.
pub const QUERY_TERMS: [TermSyntax; 9] = [
    TermSyntax {
        examples: &["due:today"],
        description: "due on that day (today, fri, +3d, -1w, 2026-12-01)",
    },
    TermSyntax {
        examples: &["due:<=fri"],
        description: "due before or after that day (<, <=, >, >=)",
    },
    TermSyntax {
        examples: &["due:overdue"],
        description: "open and due before today",
    },
    TermSyntax {
        examples: &["due:none", "due:any"],
        description: "without or with a due date",
    },
    TermSyntax {
        examples: &["created:>-1m"],
        description: "created in the last month (dates as for due)",
    },
    TermSyntax {
        examples: &["done:<=-1w"],
        description: "completed a week ago or earlier",
    },
    TermSyntax {
        examples: &["priority:>=medium"],
        description: "priority (lowest, low, medium, high, highest)",
    },
    TermSyntax {
        examples: &["#tag"],
        description: "with the tag or a tag nested in it",
    },
    TermSyntax {
        examples: &["is:open", "is:done"],
        description: "open, or done or cancelled",
    },
];

#[cfg(test)]
mod tests {
    use jiff::civil::date;
    use proptest::prelude::*;
    use rstest::rstest;

    use super::*;
    use crate::user_docs;

    /// A Tuesday.
    const TODAY: Date = date(2026, 11, 24);

    fn parse_task(line: &str) -> TaskLine {
        TaskLine::parse(line).unwrap()
    }

    fn compare(comparison: Comparison, day: i8) -> DateCondition {
        DateCondition::Compare(comparison, date(2026, 11, day))
    }

    #[rstest]
    #[case::due_keyword("due:today", Term::Date { field: DateField::Due, condition: compare(Comparison::Equal, 24) })]
    #[case::due_weekday("due:fri", Term::Date { field: DateField::Due, condition: compare(Comparison::Equal, 27) })]
    #[case::due_offset("due:+3d", Term::Date { field: DateField::Due, condition: compare(Comparison::Equal, 27) })]
    #[case::due_iso("due:2026-11-01", Term::Date { field: DateField::Due, condition: compare(Comparison::Equal, 1) })]
    #[case::due_upper_case_value("due:TODAY", Term::Date { field: DateField::Due, condition: compare(Comparison::Equal, 24) })]
    #[case::less("due:<today", Term::Date { field: DateField::Due, condition: compare(Comparison::Less, 24) })]
    #[case::less_or_equal("due:<=fri", Term::Date { field: DateField::Due, condition: compare(Comparison::LessOrEqual, 27) })]
    #[case::greater("due:>today", Term::Date { field: DateField::Due, condition: compare(Comparison::Greater, 24) })]
    #[case::greater_or_equal("due:>=-1w", Term::Date { field: DateField::Due, condition: compare(Comparison::GreaterOrEqual, 17) })]
    #[case::overdue("due:overdue", Term::Date { field: DateField::Due, condition: DateCondition::Overdue(TODAY) })]
    #[case::due_none("due:none", Term::Date { field: DateField::Due, condition: DateCondition::Missing })]
    #[case::due_any("due:ANY", Term::Date { field: DateField::Due, condition: DateCondition::Present })]
    #[case::created_back("created:>-1w", Term::Date { field: DateField::Created, condition: compare(Comparison::Greater, 17) })]
    #[case::created_none("created:none", Term::Date { field: DateField::Created, condition: DateCondition::Missing })]
    #[case::done("done:<=-1d", Term::Date { field: DateField::Done, condition: compare(Comparison::LessOrEqual, 23) })]
    #[case::priority("priority:high", Term::Priority { comparison: Comparison::Equal, level: 4 })]
    #[case::priority_compared("priority:>=Medium", Term::Priority { comparison: Comparison::GreaterOrEqual, level: 3 })]
    #[case::tag("#Home", Term::Tag("home".to_owned()))]
    #[case::nested_tag("#area/sub", Term::Tag("area/sub".to_owned()))]
    #[case::open("is:open", Term::Status(StatusFilter::Open))]
    #[case::done_status("is:DONE", Term::Status(StatusFilter::Done))]
    fn term_parses_its_value(#[case] word: &str, #[case] expected: Term) {
        let system_under_test = Term::parse(word, TODAY);

        assert_eq!(system_under_test.unwrap(), Some(expected));
    }

    #[rstest]
    #[case::plain("milk")]
    #[case::unknown_prefix("foo:bar")]
    #[case::upper_case_prefix("Due:today")]
    #[case::url("https://example.com")]
    #[case::hash_only("#")]
    #[case::numeric_tag("#123")]
    #[case::tag_with_punctuation("#home,")]
    #[case::hash_inside("C#")]
    fn term_leaves_other_words_as_text(#[case] word: &str) {
        let system_under_test = Term::parse(word, TODAY);

        assert_eq!(system_under_test.unwrap(), None);
    }

    #[rstest]
    #[case::empty_value("due:", TermKey::Due, "missing value")]
    #[case::operator_only("due:<=", TermKey::Due, "missing value")]
    #[case::unknown_date("due:blah", TermKey::Due, "unknown date expression `blah`")]
    #[case::impossible_date("created:2026-02-30", TermKey::Created, "invalid date `2026-02-30`")]
    #[case::overdue_only_for_due(
        "done:overdue",
        TermKey::Done,
        "unknown date expression `overdue`"
    )]
    #[case::compared_keyword("due:<none", TermKey::Due, "missing value")]
    #[case::unknown_priority("priority:urgent", TermKey::Priority, "unknown priority `urgent`")]
    #[case::priority_operator_only("priority:>", TermKey::Priority, "missing value")]
    #[case::unknown_status("is:later", TermKey::Is, "unknown status `later`")]
    fn term_rejects_invalid_values(
        #[case] word: &str,
        #[case] expected_key: TermKey,
        #[case] expected_reason: &str,
    ) {
        let expected_message = format!("{expected_key}: {expected_reason}");

        let system_under_test = Term::parse(word, TODAY).unwrap_err();

        assert_eq!(system_under_test.key(), expected_key);
        assert_eq!(system_under_test.reason().to_string(), expected_reason);
        assert_eq!(system_under_test.to_string(), expected_message);
    }

    #[rstest]
    #[case::due_today("due:today", "- [ ] a [due:: 2026-11-24]", true)]
    #[case::due_other_day("due:today", "- [ ] a [due:: 2026-11-25]", false)]
    #[case::without_due_date("due:today", "- [ ] a", false)]
    #[case::due_not_a_date("due:any", "- [ ] a [due:: someday]", false)]
    #[case::before("due:<=fri", "- [ ] a [due:: 2026-11-27]", true)]
    #[case::after_the_bound("due:<=fri", "- [ ] a [due:: 2026-11-28]", false)]
    #[case::strictly_after("due:>today", "- [ ] a [due:: 2026-11-24]", false)]
    #[case::overdue_open("due:overdue", "- [ ] a [due:: 2026-11-23]", true)]
    #[case::overdue_in_progress("due:overdue", "- [/] a [due:: 2026-11-23]", true)]
    #[case::overdue_done("due:overdue", "- [x] a [due:: 2026-11-23]", false)]
    #[case::due_today_is_not_overdue("due:overdue", "- [ ] a [due:: 2026-11-24]", false)]
    #[case::none_without("due:none", "- [ ] a", true)]
    #[case::none_with("due:none", "- [ ] a [due:: 2026-11-24]", false)]
    #[case::created_recently("created:>-1m", "- [ ] a [created:: 2026-11-01]", true)]
    #[case::created_long_ago("created:>-1m", "- [ ] a [created:: 2026-10-24]", false)]
    #[case::done_a_week_ago("done:<=-1w", "- [x] a [completion:: 2026-11-17]", true)]
    #[case::done_yesterday("done:<=-1w", "- [x] a [completion:: 2026-11-23]", false)]
    #[case::priority("priority:high", "- [ ] a [priority:: high]", true)]
    #[case::priority_other("priority:high", "- [ ] a [priority:: highest]", false)]
    #[case::normal_is_below_medium("priority:>=medium", "- [ ] a", false)]
    #[case::normal_is_above_low("priority:>low", "- [ ] a [priority:: urgent]", true)]
    #[case::tag("#home", "- [ ] a #Home", true)]
    #[case::nested_tag("#home", "- [ ] a #home/garden", true)]
    #[case::longer_tag("#home", "- [ ] a #homework", false)]
    #[case::parent_tag("#home/garden", "- [ ] a #home", false)]
    #[case::tag_in_code("#home", "- [ ] a `#home`", false)]
    #[case::open("is:open", "- [/] a", true)]
    #[case::open_cancelled("is:open", "- [-] a", false)]
    #[case::done("is:done", "- [-] a", true)]
    fn term_matches_tasks(#[case] word: &str, #[case] line: &str, #[case] expected: bool) {
        let term = Term::parse(word, TODAY).unwrap().unwrap();
        let task = parse_task(line);

        let system_under_test = term.matches(&task);

        assert_eq!(system_under_test, expected);
    }

    #[rstest]
    #[case::empty("", "- [ ] Buy milk", true)]
    #[case::words_in_any_order("milk buy", "- [ ] Buy milk", true)]
    #[case::smart_case_per_word("Milk", "- [ ] Buy milk", false)]
    #[case::words_and_terms("milk is:open", "- [ ] Buy milk", true)]
    #[case::term_fails("milk is:done", "- [ ] Buy milk", false)]
    #[case::text_fails("bread is:open", "- [ ] Buy milk", false)]
    #[case::fields_are_no_text("2026", "- [ ] Buy milk [due:: 2026-11-24]", false)]
    #[case::every_term("due:today priority:high", "- [ ] a [due:: 2026-11-24]", false)]
    fn query_matches_every_word(#[case] input: &str, #[case] line: &str, #[case] expected: bool) {
        let query = Query::parse(input, TODAY).unwrap();
        let task = parse_task(line);

        let system_under_test = query.matches(&task);

        assert_eq!(system_under_test, expected);
    }

    #[rstest]
    #[case::text_kept_as_typed(" buy   milk ", " buy   milk ", false)]
    #[case::term_between("buy due:today milk", "buy milk", true)]
    #[case::term_first("due:today  milk", "milk", true)]
    #[case::term_last("milk due:today", "milk", true)]
    #[case::term_last_before_a_space("milk  due:today ", "milk ", true)]
    #[case::terms_only("#home is:open", "", true)]
    fn query_separates_terms_from_text(
        #[case] input: &str,
        #[case] expected_text: &str,
        #[case] expected_has_terms: bool,
    ) {
        let system_under_test = Query::parse(input, TODAY).unwrap();

        assert_eq!(system_under_test.text(), expected_text);
        assert_eq!(system_under_test.has_terms(), expected_has_terms);
    }

    #[rstest]
    #[case::nothing("  ", true)]
    #[case::text("milk", false)]
    #[case::term("is:open", false)]
    fn query_is_empty_without_words(#[case] input: &str, #[case] expected: bool) {
        let system_under_test = Query::parse(input, TODAY).unwrap();

        assert_eq!(system_under_test.is_empty(), expected);
    }

    #[test]
    fn query_reports_the_first_invalid_term() {
        let input = "milk is:later due:blah";

        let system_under_test = Query::parse(input, TODAY).unwrap_err();

        assert_eq!(system_under_test.key(), TermKey::Is);
    }

    #[test]
    fn snapshot_matches_like_its_task() {
        let lines = [
            "- [ ] a #home [due:: 2026-11-24] [priority:: high]",
            "- [x] b [created:: 2026-11-01] [completion:: 2026-11-17]",
            "- [-] c",
        ];
        let queries = [
            "due:today",
            "#home",
            "priority:high",
            "created:any",
            "done:<=-1w",
            "is:done",
        ];
        let tasks: Vec<TaskLine> = lines.iter().map(|line| parse_task(line)).collect();
        let expected: Vec<bool> = queries
            .iter()
            .flat_map(|input| {
                let query = Query::parse(input, TODAY).unwrap();
                tasks
                    .iter()
                    .map(|task| query.matches(task))
                    .collect::<Vec<_>>()
            })
            .collect();

        let system_under_test: Vec<bool> = queries
            .iter()
            .flat_map(|input| {
                let query = Query::parse(input, TODAY).unwrap();
                tasks
                    .iter()
                    .map(|task| query.matches(&TaskSnapshot::from(task)))
                    .collect::<Vec<_>>()
            })
            .collect();

        assert_eq!(system_under_test, expected);
    }

    #[test]
    fn every_documented_example_is_a_valid_term() {
        let examples = QUERY_TERMS.iter().flat_map(TermSyntax::examples);

        let system_under_test: Vec<(&str, bool)> = examples
            .map(|example| {
                let parsed = Term::parse(example, TODAY);
                (*example, matches!(parsed, Ok(Some(_))))
            })
            .collect();

        for (example, is_term) in system_under_test {
            assert!(is_term, "{example}");
        }
    }

    #[test]
    fn every_completed_word_is_a_valid_term() {
        let words = term_words();

        let system_under_test: Vec<(&str, bool)> = words
            .map(|word| (word, matches!(Term::parse(word, TODAY), Ok(Some(_)))))
            .collect();

        assert!(system_under_test.len() > TermKey::iter().count());
        for (word, is_term) in system_under_test {
            assert!(is_term, "{word}");
        }
    }

    #[test]
    fn query_terms_table_documents_every_term_syntax() {
        let expected: Vec<(Vec<&str>, &str)> = QUERY_TERMS
            .iter()
            .map(|syntax| (syntax.examples().to_vec(), syntax.description()))
            .collect();

        let system_under_test: Vec<(Vec<&str>, &str)> =
            user_docs::table_rows(&user_docs::KEYBINDINGS.section("## Query terms"), "Terms")
                .iter()
                .map(|cells| (user_docs::code_spans(cells[0]).collect(), cells[1]))
                .collect();

        assert_eq!(system_under_test, expected);
    }

    proptest! {
        #[test]
        fn words_without_prefix_or_tag_stay_text(input in "[a-zA-Z0-9 .,!?/<>=+-]{0,40}") {
            let system_under_test = Query::parse(&input, TODAY).unwrap();

            prop_assert!(!system_under_test.has_terms());
            prop_assert_eq!(system_under_test.text(), input);
        }
    }
}

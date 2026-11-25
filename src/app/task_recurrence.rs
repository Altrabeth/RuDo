//! Setting and clearing the recurrence rule of the selected task in a prompt, and the next
//! occurrence that completing a recurring task creates.

use std::borrow::Cow;
use std::fmt::Write;

use jiff::civil::Date;

use crate::app::effect::Effect;
use crate::app::prompt::PromptKind;
use crate::app::state::App;
use crate::config::IndentStyle;
use crate::domain::recurrence::{Recurrence, RecurrenceError};
use crate::markdown::document::{InsertPosition, TaskId, TodoDocument};
use crate::markdown::task_line::{FieldKey, InlineField, TaskLine, TaskStatus};
use crate::storage::workspace::ListPath;
use crate::ui::text_input::TextInput;

/// The hint of the recurrence prompt: examples of the accepted rules.
pub(super) const RECURRENCE_HINT: &str = "every day, every 2 weeks, every month; empty clears";
/// The preview of a text that clears the recurrence rule.
const NO_RECURRENCE_PREVIEW: &str = "no recurrence";

/// Parses the recurrence rule `text`; blank text is `None` and clears the rule.
pub(super) fn parse_recurrence_text(text: &str) -> Result<Option<Recurrence>, Cow<'static, str>> {
    if text.trim().is_empty() {
        return Ok(None);
    }
    text.parse()
        .map(Some)
        .map_err(|error: RecurrenceError| Cow::Owned(error.to_string()))
}

/// Returns the rule `text` in the form it is stored in, or why it is not a supported rule.
pub(super) fn recurrence_preview(text: &str) -> Result<Cow<'static, str>, Cow<'static, str>> {
    Ok(match parse_recurrence_text(text)? {
        Some(rule) => Cow::Owned(rule.to_string()),
        None => Cow::Borrowed(NO_RECURRENCE_PREVIEW),
    })
}

/// Returns the text of the occurrence that follows `task` when it is completed on `today`, or
/// `None` if it has no recurrence rule.
///
/// The text keeps the description with its tags and every inline field except the completion
/// date; the due date is the next occurrence after the old one, or after `today` if the task
/// has no due date that parses.
///
/// # Errors
///
/// Returns the [`RecurrenceError`] of a rule that is not supported or whose next occurrence
/// is out of range.
fn next_occurrence(task: &TaskLine, today: Date) -> Option<Result<String, RecurrenceError>> {
    let rule = task.field(&FieldKey::Repeat)?.value();
    Some(
        rule.parse()
            .and_then(|rule| occurrence_text(task, rule, today)),
    )
}

/// Returns the text of the occurrence of `task` that `rule` schedules next.
fn occurrence_text(
    task: &TaskLine,
    rule: Recurrence,
    today: Date,
) -> Result<String, RecurrenceError> {
    let base = task
        .field(&FieldKey::Due)
        .and_then(|due| due.value().parse::<Date>().ok())
        .unwrap_or(today);
    let due = InlineField::new(FieldKey::Due, &rule.next_after(base)?.to_string())
        .expect("an ISO date is a valid inline field value");
    let mut text = task.description().to_owned();
    // The new due date comes last: setting the fields in order replaces an old due date in
    // its place, or appends the new one.
    for field in task
        .fields()
        .iter()
        .filter(|field| *field.key() != FieldKey::Completion)
        .chain([&due])
    {
        write!(text, " {field}").expect("writing to a String cannot fail");
    }
    Ok(text)
}

/// Inserts the occurrence with `text` directly above `task`, which therefore moves down one
/// row. The edit that completes `task` calls it, so that both are one undo step.
pub(super) fn insert_occurrence(
    document: &mut TodoDocument,
    task: TaskId,
    text: &str,
    fallback_indent: IndentStyle,
) {
    document
        .insert_task(InsertPosition::Above(task), text, fallback_indent)
        .expect("the completed task is in the document");
}

impl App {
    /// Opens the recurrence prompt for the selected task of the active list, holding the rule
    /// it already has.
    pub(super) fn open_recurrence_prompt(&mut self) {
        let Some(list) = self.lists.active().cloned() else {
            return;
        };
        let Some(&task) = self.task_outline().and_then(|outline| outline.selected()) else {
            return;
        };
        let stored = self
            .active_task(task)
            .and_then(|line| line.field(&FieldKey::Repeat))
            .map_or("", InlineField::value);
        let input = TextInput::with_value(stored);
        self.open_prompt(PromptKind::Recurrence { list, task }, input);
    }

    /// Returns the task `task` of the active list, if it is there.
    fn active_task(&self, task: TaskId) -> Option<&TaskLine> {
        let document = self.active_list()?.content().ok()?.document();
        let node = document.tasks().find(|node| node.id() == task)?;
        Some(node.task())
    }

    /// Sets the recurrence rule of `task` in the list at `list` to `recurrence`, or removes
    /// it if `recurrence` is `None`.
    pub(super) fn set_recurrence(
        &mut self,
        list: &ListPath,
        task: TaskId,
        recurrence: Option<Recurrence>,
    ) -> Vec<Effect> {
        let value = recurrence.map(|rule| rule.to_string());
        let effects = self.edit_list(list, |document| {
            document
                .set_field(task, FieldKey::Repeat, value.as_deref())
                .expect("a recurrence rule is a valid inline field value")
        });
        self.refresh_tasks();
        effects
    }

    /// Returns the text of the occurrence that marking `task` of the active list done
    /// creates, or `None` if the task is done already or does not recur.
    pub(super) fn occurrence_on_completion(
        &self,
        task: TaskId,
    ) -> Option<Result<String, RecurrenceError>> {
        let line = self.active_task(task)?;
        match line.status() {
            TaskStatus::Done => None,
            TaskStatus::Todo
            | TaskStatus::InProgress
            | TaskStatus::Cancelled
            | TaskStatus::Other(_) => next_occurrence(line, self.today),
        }
    }
}

#[cfg(test)]
mod tests {
    use jiff::civil::date;
    use rstest::rstest;

    use super::*;

    /// A Tuesday.
    const TODAY: Date = date(2026, 11, 24);

    fn task(line: &str) -> TaskLine {
        TaskLine::parse(line).unwrap()
    }

    /// Returns `line` followed by the occurrence that completing it inserts above it.
    fn with_next_occurrence(line: &str) -> String {
        let mut document = TodoDocument::parse(&format!("{line}\n"));
        let node = document.tasks().next().unwrap();
        let (id, text) = (
            node.id(),
            next_occurrence(node.task(), TODAY).unwrap().unwrap(),
        );
        insert_occurrence(&mut document, id, &text, IndentStyle::Tab);
        document.to_string()
    }

    #[rstest]
    #[case::after_the_due_date(
        "- [ ] Water [due:: 2026-11-22] [repeat:: every week]",
        "- [ ] Water [due:: 2026-11-29] [repeat:: every week]"
    )]
    #[case::after_today_without_a_due_date(
        "- [ ] Water [repeat:: every 2 days]",
        "- [ ] Water [repeat:: every 2 days] [due:: 2026-11-26]"
    )]
    #[case::after_today_with_an_unparseable_due_date(
        "- [ ] Water [due:: someday] [repeat:: every day]",
        "- [ ] Water [due:: 2026-11-25] [repeat:: every day]"
    )]
    #[case::keeps_tags_priority_and_unknown_fields(
        "1. [/] Pay #home [priority:: high] [repeat:: every month] [x:: 1] [due:: 2026-01-31]",
        "1. [ ] Pay #home [priority:: high] [repeat:: every month] [x:: 1] [due:: 2026-02-28]"
    )]
    #[case::drops_the_completion_date(
        "- [-] Run [completion:: 2026-11-20] [repeat:: every year] [due:: 2026-11-20]",
        "- [ ] Run [repeat:: every year] [due:: 2027-11-20]"
    )]
    fn next_occurrence_is_open_with_the_next_due_date(#[case] line: &str, #[case] expected: &str) {
        let expected = format!("{expected}\n{line}\n");

        let system_under_test = with_next_occurrence(line);

        assert_eq!(system_under_test, expected);
    }

    #[test]
    fn task_without_a_rule_has_no_next_occurrence() {
        let line = task("- [ ] Water [due:: 2026-11-22]");

        let system_under_test = next_occurrence(&line, TODAY);

        assert!(system_under_test.is_none());
    }

    #[rstest]
    #[case::unsupported("- [ ] Water [repeat:: every weekday]")]
    #[case::out_of_range("- [ ] Water [due:: 9999-12-31] [repeat:: every day]")]
    fn invalid_rule_is_an_error(#[case] line: &str) {
        let line = task(line);

        let system_under_test = next_occurrence(&line, TODAY);

        assert!(matches!(system_under_test, Some(Err(_))));
    }

    #[rstest]
    #[case::canonical(" Every 1 Week ", "every week")]
    #[case::plural("every 2 week", "every 2 weeks")]
    #[case::empty("  ", "no recurrence")]
    fn preview_shows_the_stored_rule(#[case] text: &str, #[case] expected: &str) {
        let system_under_test = recurrence_preview(text);

        assert_eq!(system_under_test, Ok(Cow::Borrowed(expected)));
    }

    #[rstest]
    #[case::unsupported("every weekday", "unsupported recurrence rule")]
    #[case::missing_every("weekly", "recurrence rule must start with `every`")]
    fn preview_of_an_invalid_rule_is_its_error(#[case] text: &str, #[case] expected: &str) {
        let system_under_test = recurrence_preview(text);

        assert_eq!(system_under_test, Err(Cow::Borrowed(expected)));
    }
}

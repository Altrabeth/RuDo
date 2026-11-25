//! Setting and clearing the due date of the selected task in a prompt with a live preview.

use std::borrow::Cow;

use jiff::civil::Date;

use crate::app::effect::Effect;
use crate::app::prompt::PromptKind;
use crate::app::state::App;
use crate::domain::date::{RelativeDue, parse_due};
use crate::markdown::document::TaskId;
use crate::markdown::task_line::FieldKey;
use crate::storage::workspace::ListPath;

/// The hint of the due date prompt: examples of the accepted expressions.
pub(super) const DUE_DATE_HINT: &str = "today, tomorrow, +3d, fri, 2026-12-24; empty clears";
/// The preview of an expression that clears the due date.
const NO_DUE_DATE_PREVIEW: &str = "no due date";

/// Parses the due date expression `text` relative to `today`; `None` clears the due date.
pub(super) fn parse_due_text(text: &str, today: Date) -> Result<Option<Date>, Cow<'static, str>> {
    parse_due(text, today).map_err(|error| Cow::Owned(error.to_string()))
}

/// Returns the date that `text` names relative to `today`, as in `Mon 2026-11-30, in 6d`, or
/// why `text` is not a due date expression.
pub(super) fn due_preview(text: &str, today: Date) -> Result<Cow<'static, str>, Cow<'static, str>> {
    Ok(match parse_due_text(text, today)? {
        Some(due) => Cow::Owned(format!(
            "{}, {}",
            due.strftime("%a %Y-%m-%d"),
            RelativeDue::new(due, today)
        )),
        None => Cow::Borrowed(NO_DUE_DATE_PREVIEW),
    })
}

impl App {
    /// Opens the due date prompt for the selected task of the active list, holding the due
    /// date it already has.
    pub(super) fn open_due_date_prompt(&mut self) {
        self.open_field_prompt(&FieldKey::Due, |list, task| PromptKind::DueDate {
            list,
            task,
        });
    }

    /// Sets the due date of `task` in the list at `list` to `due`, or removes it if `due` is
    /// `None`.
    pub(super) fn set_due(
        &mut self,
        list: &ListPath,
        task: TaskId,
        due: Option<Date>,
    ) -> Vec<Effect> {
        let value = due.map(|due| due.to_string());
        self.set_task_field(list, task, FieldKey::Due, value.as_deref())
    }
}

#[cfg(test)]
mod tests {
    use jiff::civil::date;
    use rstest::rstest;

    use super::*;

    /// A Tuesday.
    const TODAY: Date = date(2026, 11, 24);

    #[rstest]
    #[case::weekday("mon", "Mon 2026-11-30, in 6d")]
    #[case::today("today", "Tue 2026-11-24, today")]
    #[case::past("2026-11-20", "Fri 2026-11-20, 4d overdue")]
    #[case::empty("", "no due date")]
    #[case::none(" none ", "no due date")]
    fn preview_shows_the_date_and_its_distance(#[case] text: &str, #[case] expected: &str) {
        let system_under_test = due_preview(text, TODAY);

        assert_eq!(system_under_test, Ok(Cow::Borrowed(expected)));
    }

    #[test]
    fn preview_of_an_unknown_expression_is_its_error() {
        let expected = "unknown date expression `someday`";

        let system_under_test = due_preview(" someday", TODAY);

        assert_eq!(system_under_test, Err(Cow::Borrowed(expected)));
    }

    #[rstest]
    #[case::date("+3d", Some(date(2026, 11, 27)))]
    #[case::clear("", None)]
    fn text_parses_relative_to_today(#[case] text: &str, #[case] expected: Option<Date>) {
        let system_under_test = parse_due_text(text, TODAY);

        assert_eq!(system_under_test, Ok(expected));
    }

    #[test]
    fn invalid_date_becomes_its_reason() {
        let expected = "invalid date `2026-02-30`";

        let system_under_test = parse_due_text("2026-02-30", TODAY);

        assert_eq!(system_under_test, Err(Cow::Borrowed(expected)));
    }
}

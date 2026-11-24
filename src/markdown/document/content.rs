//! Content edits of a single task: its description, its status and its inline fields.
//!
//! Each edit runs on a copy of the task line and is applied only if it changes the task, so
//! a no-op keeps the line's verbatim rendering and reports [`EditOutcome::Unchanged`].

use jiff::civil::Date;

use super::{EditOutcome, LineKind, TaskId, TodoDocument};
use crate::markdown::task_line::{FieldKey, InlineField, InlineFieldError, TaskLine, TaskStatus};

/// Characters that break a line. A task is a single line, so typed breaks become spaces.
const LINE_BREAKS: [char; 2] = ['\r', '\n'];

impl TodoDocument {
    /// Sets the description of a task from text a user typed.
    ///
    /// Every run of line breaks becomes one space and surrounding whitespace is removed.
    /// Inline fields typed in `text` become fields as with [`TodoDocument::set_field`]; fields
    /// with other keys, the status and the tags in the text are kept.
    ///
    /// Returns [`EditOutcome::Unchanged`] if the task already has this description and these
    /// fields, or if `id` is not in this document.
    ///
    /// # Examples
    ///
    /// ```
    /// use rudo::markdown::document::{EditOutcome, TodoDocument};
    ///
    /// let mut document = TodoDocument::parse("- [ ] old [estimate:: 2h]\n");
    /// let id = document.tasks().next().expect("one task").id();
    ///
    /// let outcome = document.set_description(id, " Buy\nmilk [due:: 2026-12-01] ");
    ///
    /// assert_eq!(outcome, EditOutcome::Changed);
    /// assert_eq!(
    ///     document.to_string(),
    ///     "- [ ] Buy milk [estimate:: 2h] [due:: 2026-12-01]\n"
    /// );
    /// ```
    pub fn set_description(&mut self, id: TaskId, text: &str) -> EditOutcome {
        let text = join_lines(text);
        self.edit_task(id, |task| task.set_description(&text))
    }

    /// Toggles a task between done and todo.
    ///
    /// A task that is not done (todo, in progress, cancelled or a custom status) becomes done
    /// and, if `completion_date` is given, gets the field `[completion:: YYYY-MM-DD]`. A done
    /// task becomes todo and loses every completion field. Other fields, tags and the
    /// statuses of parents and subtasks are left untouched.
    ///
    /// Returns [`EditOutcome::Unchanged`] only if `id` is not in this document.
    ///
    /// # Examples
    ///
    /// ```
    /// use jiff::civil::date;
    /// use rudo::markdown::document::{EditOutcome, TodoDocument};
    ///
    /// let mut document = TodoDocument::parse("- [ ] Buy milk #home");
    /// let id = document.tasks().next().expect("one task").id();
    ///
    /// let outcome = document.toggle_done(id, Some(date(2026, 11, 24)));
    /// assert_eq!(outcome, EditOutcome::Changed);
    /// assert_eq!(document.to_string(), "- [x] Buy milk #home [completion:: 2026-11-24]");
    ///
    /// let outcome = document.toggle_done(id, None);
    /// assert_eq!(outcome, EditOutcome::Changed);
    /// assert_eq!(document.to_string(), "- [ ] Buy milk #home");
    /// ```
    pub fn toggle_done(&mut self, id: TaskId, completion_date: Option<Date>) -> EditOutcome {
        self.edit_task(id, |task| {
            if task.status() == TaskStatus::Done {
                mark_todo(task);
            } else {
                mark_done(task, completion_date);
            }
        })
    }

    /// Sets the inline field `key` of a task to `value`, or removes it if `value` is `None`.
    ///
    /// Setting replaces the first field with this key in place, or appends the field if there
    /// is none. Other fields with the same key are removed, so the task ends up with exactly
    /// one. Removing removes every field with this key.
    ///
    /// Returns [`EditOutcome::Unchanged`] if the task already has exactly this field (or no
    /// field with this key when removing), or if `id` is not in this document.
    ///
    /// # Errors
    ///
    /// Returns an [`InlineFieldError`] if `key` and `value` do not form a valid field, see
    /// [`InlineField::new`]. The document is not changed then.
    ///
    /// # Examples
    ///
    /// ```
    /// use rudo::markdown::document::{EditOutcome, TodoDocument};
    /// use rudo::markdown::task_line::{FieldKey, InlineFieldError};
    ///
    /// let mut document = TodoDocument::parse("- [ ] a [due:: 1] [priority:: low] [due:: 2]");
    /// let id = document.tasks().next().expect("one task").id();
    ///
    /// let outcome = document.set_field(id, FieldKey::Due, Some("2026-12-01"))?;
    ///
    /// assert_eq!(outcome, EditOutcome::Changed);
    /// assert_eq!(document.to_string(), "- [ ] a [due:: 2026-12-01] [priority:: low]");
    /// # Ok::<(), InlineFieldError>(())
    /// ```
    pub fn set_field(
        &mut self,
        id: TaskId,
        key: FieldKey,
        value: Option<&str>,
    ) -> Result<EditOutcome, InlineFieldError> {
        let Some(value) = value else {
            return Ok(self.edit_task(id, |task| remove_fields(task, &key)));
        };
        let field = InlineField::new(key, value)?;
        Ok(self.edit_task(id, |task| task.set_field(field)))
    }

    /// Applies `edit` to the task `id` and reports whether that changed its content.
    fn edit_task(&mut self, id: TaskId, edit: impl FnOnce(&mut TaskLine)) -> EditOutcome {
        let Some(task) = self.task_mut(id) else {
            return EditOutcome::Unchanged;
        };
        // The mutators mark a task as modified even if they set what is already there, which
        // would replace its verbatim rendering. Editing a copy leaves a no-op without trace.
        let mut edited = task.clone();
        edit(&mut edited);
        if has_same_content(task, &edited) {
            return EditOutcome::Unchanged;
        }
        *task = edited;
        EditOutcome::Changed
    }

    fn task_mut(&mut self, id: TaskId) -> Option<&mut TaskLine> {
        self.lines.iter_mut().find_map(|line| match &mut line.kind {
            LineKind::Task(task_id, task) if *task_id == id => Some(task),
            LineKind::Task(..) | LineKind::Raw(_) => None,
        })
    }
}

/// Returns whether two versions of a task render the same once both are modified. The
/// content edits never change the indentation or the list marker.
fn has_same_content(task: &TaskLine, other: &TaskLine) -> bool {
    task.status() == other.status()
        && task.description() == other.description()
        && task.fields() == other.fields()
}

/// Joins the lines of `text` with single spaces, dropping empty lines.
fn join_lines(text: &str) -> String {
    let mut line = String::with_capacity(text.len());
    for part in text.split(LINE_BREAKS).filter(|part| !part.is_empty()) {
        if !line.is_empty() {
            line.push(' ');
        }
        line.push_str(part);
    }
    line
}

fn mark_done(task: &mut TaskLine, completion_date: Option<Date>) {
    task.set_status(TaskStatus::Done);
    if let Some(date) = completion_date {
        let completion = InlineField::new(FieldKey::Completion, &date.to_string())
            .expect("an ISO 8601 date is a valid field value");
        task.set_field(completion);
    }
}

fn mark_todo(task: &mut TaskLine) {
    task.set_status(TaskStatus::Todo);
    remove_fields(task, &FieldKey::Completion);
}

/// Removes every field with the given key.
fn remove_fields(task: &mut TaskLine, key: &FieldKey) {
    while task.remove_field(key).is_some() {}
}

#[cfg(test)]
mod tests {
    use jiff::civil::date;
    use rstest::rstest;
    use strum::IntoEnumIterator;

    use super::*;

    const UNKNOWN: TaskId = TaskId(99);

    const COMPLETION_DATE: Date = date(2026, 11, 24);

    fn first_task(document: &TodoDocument) -> TaskId {
        document.tasks().next().expect("a task").id()
    }

    fn other_key(name: &str) -> FieldKey {
        FieldKey::Other(name.to_owned())
    }

    #[rstest]
    #[case::plain("- [ ] old", "new", "- [ ] new")]
    #[case::trims_whitespace("- [ ] old", " \t new \t ", "- [ ] new")]
    #[case::joins_lf_lines("- [ ] old", "Buy\nfresh\n\nmilk\n", "- [ ] Buy fresh milk")]
    #[case::joins_crlf_lines("- [ ] old", "Buy\r\nmilk\r\n", "- [ ] Buy milk")]
    #[case::joins_cr_lines("- [ ] old", "Buy\rmilk", "- [ ] Buy milk")]
    #[case::keeps_status("- [/] old", "new", "- [/] new")]
    #[case::keeps_tags_in_text("- [ ] old #home", "new #work", "- [ ] new #work")]
    #[case::keeps_other_fields(
        "- [ ] old [estimate:: 2h]",
        "new [due:: 2026-12-01]",
        "- [ ] new [estimate:: 2h] [due:: 2026-12-01]"
    )]
    #[case::typed_field_replaces_duplicates(
        "- [ ] old [due:: 1] [priority:: low] [due:: 2]",
        "old [due:: 3]",
        "- [ ] old [due:: 3] [priority:: low]"
    )]
    #[case::field_split_over_lines(
        "- [ ] old",
        "new [due::\n2026-12-01]",
        "- [ ] new [due:: 2026-12-01]"
    )]
    #[case::multi_byte(
        "- [ ] alt",
        "Grüße an Jürgen\n日本語 [ort:: Köln]",
        "- [ ] Grüße an Jürgen 日本語 [ort:: Köln]"
    )]
    #[case::keeps_children("- [ ] old\n  - [ ] child", "new", "- [ ] new\n  - [ ] child")]
    fn set_description_changes_task(
        #[case] text: &str,
        #[case] description: &str,
        #[case] expected_text: &str,
    ) {
        let mut document = TodoDocument::parse(text);
        let id = first_task(&document);

        let system_under_test = (document.set_description(id, description), document);

        assert_eq!(system_under_test.0, EditOutcome::Changed);
        assert_eq!(system_under_test.1.to_string(), expected_text);
    }

    #[rstest]
    #[case::same_text("- [ ] Buy milk", "Buy milk")]
    #[case::surrounding_whitespace("-  [ ] Buy milk ", "\n Buy milk\r\n")]
    #[case::same_typed_field("- [ ] Buy milk [due:: 1] [estimate:: 2h]", "Buy milk [due:: 1]")]
    #[case::multi_byte("- [x] Grüße 日本語 [ort:: Köln]", "Grüße 日本語 [ort:: Köln]")]
    fn set_description_to_same_text_is_unchanged(#[case] text: &str, #[case] description: &str) {
        let mut document = TodoDocument::parse(text);
        let id = first_task(&document);

        let system_under_test = (document.set_description(id, description), document);

        assert_eq!(system_under_test.0, EditOutcome::Unchanged);
        assert_eq!(system_under_test.1.to_string(), text);
    }

    #[test]
    fn toggle_done_flips_every_status() {
        let cases: Vec<(String, TaskStatus)> = TaskStatus::iter()
            .map(|status| {
                let expected = match status {
                    TaskStatus::Done => TaskStatus::Todo,
                    TaskStatus::Todo
                    | TaskStatus::InProgress
                    | TaskStatus::Cancelled
                    | TaskStatus::Other(_) => TaskStatus::Done,
                };
                (format!("- [{}] a", status.as_char()), expected)
            })
            .collect();

        let system_under_test: Vec<(EditOutcome, TaskStatus)> = cases
            .iter()
            .map(|(text, _)| {
                let mut document = TodoDocument::parse(text);
                let id = first_task(&document);
                let outcome = document.toggle_done(id, None);
                (
                    outcome,
                    document.tasks().next().expect("a task").task().status(),
                )
            })
            .collect();

        let expected: Vec<(EditOutcome, TaskStatus)> = cases
            .iter()
            .map(|&(_, status)| (EditOutcome::Changed, status))
            .collect();
        assert_eq!(system_under_test, expected);
    }

    #[rstest]
    #[case::todo_with_date(
        "- [ ] a #tag [estimate:: 2h]",
        Some(COMPLETION_DATE),
        "- [x] a #tag [estimate:: 2h] [completion:: 2026-11-24]"
    )]
    #[case::todo_without_date("- [ ] a [estimate:: 2h]", None, "- [x] a [estimate:: 2h]")]
    #[case::in_progress("- [/] a", Some(COMPLETION_DATE), "- [x] a [completion:: 2026-11-24]")]
    #[case::cancelled_replaces_stale_completion(
        "- [-] a [completion:: 2026-01-01] [estimate:: 2h]",
        Some(COMPLETION_DATE),
        "- [x] a [completion:: 2026-11-24] [estimate:: 2h]"
    )]
    #[case::custom_status("- [?] a", Some(COMPLETION_DATE), "- [x] a [completion:: 2026-11-24]")]
    #[case::done_loses_completion(
        "- [x] a [completion:: 2026-01-01] #tag [estimate:: 2h]",
        Some(COMPLETION_DATE),
        "- [ ] a #tag [estimate:: 2h]"
    )]
    #[case::done_loses_every_completion(
        "- [X] a [completion:: 1] [due:: 2] [completion:: 3]",
        None,
        "- [ ] a [due:: 2]"
    )]
    #[case::multi_byte(
        "- [ ] Äpfel 日本語 [ort:: Köln]",
        Some(COMPLETION_DATE),
        "- [x] Äpfel 日本語 [ort:: Köln] [completion:: 2026-11-24]"
    )]
    #[case::does_not_cascade(
        "- [ ] parent\n  - [ ] child\n",
        None,
        "- [x] parent\n  - [ ] child\n"
    )]
    fn toggle_done_changes_status_and_completion(
        #[case] text: &str,
        #[case] completion_date: Option<Date>,
        #[case] expected_text: &str,
    ) {
        let mut document = TodoDocument::parse(text);
        let id = first_task(&document);

        let system_under_test = (document.toggle_done(id, completion_date), document);

        assert_eq!(system_under_test.0, EditOutcome::Changed);
        assert_eq!(system_under_test.1.to_string(), expected_text);
    }

    #[rstest]
    #[case::appends(
        "- [ ] a",
        FieldKey::Due,
        Some("2026-12-01"),
        "- [ ] a [due:: 2026-12-01]"
    )]
    #[case::replaces_in_place(
        "- [ ] a [due:: 1] [priority:: low]",
        FieldKey::Due,
        Some("2"),
        "- [ ] a [due:: 2] [priority:: low]"
    )]
    #[case::replaces_first_and_removes_others(
        "- [ ] a [due:: 1] [priority:: low] [due:: 3]",
        FieldKey::Due,
        Some("2"),
        "- [ ] a [due:: 2] [priority:: low]"
    )]
    #[case::removes_duplicates_of_same_value(
        "- [ ] a [due:: 1] [due:: 1]",
        FieldKey::Due,
        Some("1"),
        "- [ ] a [due:: 1]"
    )]
    #[case::other_key(
        "- [ ] a #tag",
        other_key("estimate"),
        Some(" 2h "),
        "- [ ] a #tag [estimate:: 2h]"
    )]
    #[case::removes_every_occurrence(
        "- [ ] a [due:: 1] [priority:: low] [due:: 2]",
        FieldKey::Due,
        None,
        "- [ ] a [priority:: low]"
    )]
    #[case::multi_byte(
        "- [ ] Grüße [ort:: Köln]",
        other_key("ort"),
        Some("東京"),
        "- [ ] Grüße [ort:: 東京]"
    )]
    fn set_field_changes_fields(
        #[case] text: &str,
        #[case] key: FieldKey,
        #[case] value: Option<&str>,
        #[case] expected_text: &str,
    ) {
        let mut document = TodoDocument::parse(text);
        let id = first_task(&document);

        let system_under_test = (document.set_field(id, key, value), document);

        assert_eq!(system_under_test.0, Ok(EditOutcome::Changed));
        assert_eq!(system_under_test.1.to_string(), expected_text);
    }

    #[rstest]
    #[case::same_value("-  [ ] a [due:: 1] [priority:: low] ", FieldKey::Due, Some(" 1 "))]
    #[case::multi_byte("- [ ] a [ort:: Köln]", other_key("ort"), Some("Köln"))]
    #[case::missing_field_removed("- [ ] a [due:: 1]", FieldKey::Priority, None)]
    fn set_field_to_current_state_is_unchanged(
        #[case] text: &str,
        #[case] key: FieldKey,
        #[case] value: Option<&str>,
    ) {
        let mut document = TodoDocument::parse(text);
        let id = first_task(&document);

        let system_under_test = (document.set_field(id, key, value), document);

        assert_eq!(system_under_test.0, Ok(EditOutcome::Unchanged));
        assert_eq!(system_under_test.1.to_string(), text);
    }

    #[test]
    fn set_field_rejects_invalid_value_without_changing_document() {
        let text = "- [ ] a [due:: 1]";
        let mut document = TodoDocument::parse(text);
        let id = first_task(&document);
        let expected_error = InlineFieldError::InvalidValue("x]".to_owned());

        let system_under_test = (document.set_field(id, FieldKey::Due, Some("x]")), document);

        assert_eq!(system_under_test.0, Err(expected_error));
        assert_eq!(system_under_test.1.to_string(), text);
    }

    #[test]
    fn edits_of_unknown_task_are_unchanged() {
        let text = "- [ ] a [due:: 1]";
        let mut document = TodoDocument::parse(text);
        let outcomes = [
            document.set_description(UNKNOWN, "b"),
            document.toggle_done(UNKNOWN, Some(COMPLETION_DATE)),
            document
                .set_field(UNKNOWN, FieldKey::Due, Some("2"))
                .expect("a valid field"),
            document
                .set_field(UNKNOWN, FieldKey::Due, None)
                .expect("a valid field"),
        ];

        let system_under_test = (outcomes, document);

        assert_eq!(system_under_test.0, [EditOutcome::Unchanged; 4]);
        assert_eq!(system_under_test.1.to_string(), text);
    }
}

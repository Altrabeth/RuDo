//! Reading and writing the inline fields of a task, shared by the handlers that edit task
//! metadata: due dates, recurrence rules and priorities.

use crate::app::effect::Effect;
use crate::app::prompt::PromptKind;
use crate::app::state::App;
use crate::markdown::document::TaskId;
use crate::markdown::task_line::{FieldKey, InlineField, TaskLine};
use crate::storage::workspace::ListPath;
use crate::ui::text_input::TextInput;

impl App {
    /// Returns the active list and its selected task, or `None` if no task is selected.
    ///
    /// The path is cloned because [`App::edit_list`] borrows the whole app mutably.
    pub(super) fn selected_task_in_active_list(&self) -> Option<(ListPath, TaskId)> {
        let task = *self.task_outline()?.selected()?;
        Some((self.lists.active()?.clone(), task))
    }

    /// Returns the task `task` of the active list, if it is there.
    pub(super) fn active_task(&self, task: TaskId) -> Option<&TaskLine> {
        let document = self.active_list()?.content().ok()?.document();
        let node = document.tasks().find(|node| node.id() == task)?;
        Some(node.task())
    }

    /// Opens the prompt `prompt` builds for the selected task of the active list, holding the
    /// value the task stores for `field`.
    pub(super) fn open_field_prompt(
        &mut self,
        field: &FieldKey,
        prompt: impl FnOnce(ListPath, TaskId) -> PromptKind,
    ) {
        let Some((list, task)) = self.selected_task_in_active_list() else {
            return;
        };
        let stored = self
            .active_task(task)
            .and_then(|line| line.field(field))
            .map_or("", InlineField::value);
        let input = TextInput::with_value(stored);
        self.open_prompt(prompt(list, task), input);
    }

    /// Sets `field` of `task` in the list at `list` to `value`, or removes it if `value` is
    /// `None`, and refreshes the tasks pane.
    ///
    /// `value` must come from a parsed type such as a date or a recurrence rule, whose text is
    /// always a valid inline field value.
    pub(super) fn set_task_field(
        &mut self,
        list: &ListPath,
        task: TaskId,
        field: FieldKey,
        value: Option<&str>,
    ) -> Vec<Effect> {
        let effects = self.edit_list(list, |document| {
            document
                .set_field(task, field, value)
                .expect("values of parsed dates and rules are valid inline field values")
        });
        self.refresh_tasks();
        effects
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::TempDir;

    use super::*;
    use crate::app::state::tests::app_in;

    #[test]
    fn no_task_is_selected_without_a_list() {
        let directory = TempDir::new().unwrap();
        let app = app_in(directory.path());

        let system_under_test = app.selected_task_in_active_list();

        assert_eq!(system_under_test, None);
    }

    #[test]
    fn selected_task_of_the_opened_list_is_found_with_its_line() {
        let directory = TempDir::new().unwrap();
        fs::write(
            directory.path().join("plan.md"),
            "- [ ] Write [due:: 2026-03-20]\n",
        )
        .unwrap();
        let app = app_in(directory.path());
        let expected_due = Some("2026-03-20");

        let system_under_test = app
            .selected_task_in_active_list()
            .and_then(|(_, task)| app.active_task(task));

        assert_eq!(
            system_under_test
                .and_then(|line| line.field(&FieldKey::Due))
                .map(InlineField::value),
            expected_due
        );
    }

    #[test]
    fn setting_a_field_to_none_removes_it() {
        let directory = TempDir::new().unwrap();
        fs::write(
            directory.path().join("plan.md"),
            "- [ ] Write [due:: 2026-03-20]\n",
        )
        .unwrap();
        let mut app = app_in(directory.path());
        let (list, task) = app.selected_task_in_active_list().unwrap();

        let system_under_test = app.set_task_field(&list, task, FieldKey::Due, None);

        assert_eq!(system_under_test, vec![Effect::Save(list)]);
        assert_eq!(app.active_task(task).unwrap().field(&FieldKey::Due), None);
    }
}

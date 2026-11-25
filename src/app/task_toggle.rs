//! Toggling the selected task between done and open.

use crate::app::effect::Effect;
use crate::app::state::App;
use crate::app::status::StatusMessage;
use crate::app::task_recurrence::insert_occurrence;

impl App {
    /// Marks the selected task of the active list as done, or open again if it is done.
    ///
    /// Only the selected task changes; its subtasks and parents keep their status. With
    /// `tasks.record_completion_date` set, a task marked done gets today's date as its
    /// completion date.
    ///
    /// Completing a task with a recurrence rule inserts its next occurrence above it in the
    /// same edit, so that one undo removes both; a rule that does not work only shows why.
    pub(super) fn toggle_done(&mut self) -> Vec<Effect> {
        let Some(list) = self.lists.active().cloned() else {
            return Vec::new();
        };
        let Some(&task) = self.task_outline().and_then(|outline| outline.selected()) else {
            return Vec::new();
        };
        let completion_date = self
            .config
            .tasks()
            .record_completion_date()
            .then_some(self.today);
        let occurrence = self.occurrence_on_completion(task);
        let created = self.created_date();
        let fallback_indent = self.config.tasks().indent();
        let effects = self.edit_list(&list, |document| {
            let outcome = document.toggle_done(task, completion_date);
            if let Some(Ok(text)) = &occurrence {
                insert_occurrence(document, task, text, created, fallback_indent);
            }
            outcome
        });
        if let Some(Err(error)) = occurrence {
            self.show_message(StatusMessage::error(format!(
                "{error}: no new occurrence was created"
            )));
        }
        self.refresh_tasks();
        effects
    }
}

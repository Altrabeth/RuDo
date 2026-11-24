//! Toggling the selected task between done and open.

use crate::app::effect::Effect;
use crate::app::state::App;

impl App {
    /// Marks the selected task of the active list as done, or open again if it is done.
    ///
    /// Only the selected task changes; its subtasks and parents keep their status. With
    /// `tasks.record_completion_date` set, a task marked done gets today's date as its
    /// completion date.
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
        let effects = self.edit_list(&list, |document| {
            document.toggle_done(task, completion_date)
        });
        self.refresh_tasks();
        effects
    }
}

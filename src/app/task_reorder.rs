//! Restructuring the task tree of the active list: moving the selected task among its
//! siblings and indenting or outdenting it, always together with its subtasks.

use crate::app::effect::Effect;
use crate::app::state::App;
use crate::markdown::document::{EditOutcome, TaskId, TodoDocument};

impl App {
    /// Swaps the selected task with its next sibling.
    pub(super) fn move_task_down(&mut self) -> Vec<Effect> {
        self.restructure_selected_task(TodoDocument::move_down)
    }

    /// Swaps the selected task with its previous sibling.
    pub(super) fn move_task_up(&mut self) -> Vec<Effect> {
        self.restructure_selected_task(TodoDocument::move_up)
    }

    /// Makes the selected task the last subtask of its previous sibling, indented as configured
    /// if the list has no indentation yet.
    pub(super) fn indent_task(&mut self) -> Vec<Effect> {
        let fallback_indent = self.config.tasks().indent();
        self.restructure_selected_task(|document, task| document.indent(task, fallback_indent))
    }

    /// Moves the selected task one level up.
    pub(super) fn outdent_task(&mut self) -> Vec<Effect> {
        self.restructure_selected_task(TodoDocument::outdent)
    }

    /// Applies `restructure` to the selected task of the active list and keeps the task
    /// selected and visible wherever it went.
    fn restructure_selected_task(
        &mut self,
        restructure: impl FnOnce(&mut TodoDocument, TaskId) -> EditOutcome,
    ) -> Vec<Effect> {
        let Some(&task) = self.task_outline().and_then(|outline| outline.selected()) else {
            return Vec::new();
        };
        // `edit_list` borrows the whole app mutably, so the path cannot stay borrowed from it.
        let Some(list) = self.lists.active().cloned() else {
            return Vec::new();
        };
        let effects = self.edit_list(&list, |document| restructure(document, task));
        self.select_task(task);
        effects
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::TempDir;

    use crate::app::state::tests::app_in;

    #[test]
    fn restructuring_without_an_active_list_has_no_effect() {
        let directory = TempDir::new().unwrap();
        let mut app = app_in(directory.path());

        let system_under_test = [
            app.move_task_down(),
            app.move_task_up(),
            app.indent_task(),
            app.outdent_task(),
        ];

        assert!(system_under_test.iter().all(Vec::is_empty));
    }

    #[test]
    fn restructuring_at_a_boundary_has_no_effect() {
        let directory = TempDir::new().unwrap();
        fs::write(directory.path().join("plan.md"), "- [ ] only\n").unwrap();
        let mut app = app_in(directory.path());

        let system_under_test = [
            app.move_task_down(),
            app.move_task_up(),
            app.indent_task(),
            app.outdent_task(),
        ];

        assert!(system_under_test.iter().all(Vec::is_empty));
    }
}

//! Restructuring the task tree of the active list: moving the selected task among its
//! siblings and indenting or outdenting it or the tasks of the visual selection, always
//! together with their subtasks.

use std::collections::HashSet;

use crate::app::effect::Effect;
use crate::app::search::PaneFilter;
use crate::app::state::{App, Focus};
use crate::app::status::StatusMessage;
use crate::config::keymap::action::Action;
use crate::markdown::document::{EditOutcome, TaskId, TodoDocument};
use crate::ui::pane_message::key_sentence;

/// Shown when `indent` is used on a selection of lists and folders.
const ONLY_TASKS_ARE_INDENTED: &str = "only tasks can be indented";
/// Shown when `outdent` is used on a selection of lists and folders.
const ONLY_TASKS_ARE_OUTDENTED: &str = "only tasks can be outdented";

impl App {
    /// Swaps the selected task with its next sibling.
    pub(super) fn move_task_down(&mut self) -> Vec<Effect> {
        self.move_selected_task(TodoDocument::move_down)
    }

    /// Swaps the selected task with its previous sibling.
    pub(super) fn move_task_up(&mut self) -> Vec<Effect> {
        self.move_selected_task(TodoDocument::move_up)
    }

    /// Makes the selected task the last subtask of its previous sibling, indented as configured
    /// if the list has no indentation yet.
    pub(super) fn indent_task(&mut self) -> Vec<Effect> {
        let fallback_indent = self.config.tasks().indent();
        self.move_selected_task(|document, task| document.indent(task, fallback_indent))
    }

    /// Moves the selected task one level up.
    pub(super) fn outdent_task(&mut self) -> Vec<Effect> {
        self.move_selected_task(TodoDocument::outdent)
    }

    /// Indents the tasks of the visual selection together, as
    /// [`TodoDocument::indent_blocks`] does.
    pub(super) fn indent_selection(&mut self) -> Vec<Effect> {
        let fallback_indent = self.config.tasks().indent();
        self.restructure_selection(ONLY_TASKS_ARE_INDENTED, |document, tasks| {
            document.indent_blocks(tasks, fallback_indent)
        })
    }

    /// Outdents the tasks of the visual selection together.
    pub(super) fn outdent_selection(&mut self) -> Vec<Effect> {
        self.restructure_selection(ONLY_TASKS_ARE_OUTDENTED, TodoDocument::outdent_blocks)
    }

    /// Moves the selected task with `restructure`, unless a search filter is active.
    fn move_selected_task(
        &mut self,
        restructure: impl FnOnce(&mut TodoDocument, TaskId) -> EditOutcome,
    ) -> Vec<Effect> {
        if self.refuse_while_filtered() {
            return Vec::new();
        }
        self.restructure_selected_task(restructure)
    }

    /// Ends visual mode and applies `restructure` to the selected tasks in one edit, unless a
    /// search filter is active; in the lists pane `only_tasks` explains that nothing moves.
    fn restructure_selection(
        &mut self,
        only_tasks: &'static str,
        restructure: impl FnOnce(&mut TodoDocument, &HashSet<TaskId>) -> EditOutcome,
    ) -> Vec<Effect> {
        match self.focus {
            Focus::Lists => {
                self.show_message(StatusMessage::info(only_tasks));
                Vec::new()
            }
            Focus::Tasks => {
                let Some((list, tasks)) = self.take_selected_tasks() else {
                    return Vec::new();
                };
                if self.refuse_while_filtered() {
                    return Vec::new();
                }
                // Selected again after the edit, which may have moved it under a folded task.
                let first = tasks[0];
                let tasks: HashSet<TaskId> = tasks.into_iter().collect();
                let effects = self.edit_list(&list, |document| restructure(document, &tasks));
                self.select_task(first);
                effects
            }
        }
    }

    /// Shows a message asking to clear the search filter of the tasks pane and returns `true`
    /// if one is active: moved tasks would move among tasks the user cannot see.
    fn refuse_while_filtered(&mut self) -> bool {
        let filtered = self.task_filter().is_some_and(PaneFilter::is_active);
        if filtered {
            let message = key_sentence(&self.keymap, Action::ClearFilter, |keys| {
                format!("press {keys} to clear the filter before moving tasks")
            });
            self.show_message(StatusMessage::error(message));
        }
        filtered
    }

    /// Applies `restructure` to the selected task of the active list and keeps the task
    /// selected and visible wherever it went.
    pub(super) fn restructure_selected_task(
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

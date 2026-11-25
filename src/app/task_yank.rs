//! Yanking the selected task with its subtasks into the register and pasting the register
//! next to the selected task, also in another list.

use std::mem;

use crate::app::effect::Effect;
use crate::app::state::App;
use crate::app::status::StatusMessage;
use crate::app::task_delete::counted_tasks;
use crate::markdown::document::{EditOutcome, InsertPosition, TaskId};

/// Shown when pasting before anything was yanked or deleted.
const NOTHING_TO_PASTE: &str = "nothing to paste";

impl App {
    /// Copies the selected task of the active list with its block into the register and
    /// reports how many tasks it holds. Nothing happens without a selected task.
    pub(super) fn yank_task(&mut self) {
        let Some(&task) = self.task_outline().and_then(|outline| outline.selected()) else {
            return;
        };
        let Some(block) = self
            .active_list()
            .and_then(|list| list.content().ok())
            .and_then(|loaded| loaded.document().copy_block(task))
        else {
            return;
        };
        let message = format!("yanked {}", counted_tasks(block.task_count()));
        self.register.fill(block);
        self.show_message(StatusMessage::info(message));
    }

    /// Pastes the register at `position` relative to the selected task of the active list, or
    /// at its end without one, with one [`App::edit_list`] call and selects the pasted task.
    pub(super) fn paste(&mut self, position: fn(TaskId) -> InsertPosition) -> Vec<Effect> {
        let Some(list) = self.lists.active().cloned() else {
            return Vec::new();
        };
        let position = self.insert_position(position);
        let fallback_indent = self.config.tasks().indent();
        // `edit_list` borrows the whole app mutably, so the block cannot stay borrowed from the
        // register; it is put back below.
        let register = mem::take(&mut self.register);
        let Some(block) = register.block() else {
            self.show_message(StatusMessage::info(NOTHING_TO_PASTE));
            return Vec::new();
        };
        let mut pasted = None;
        let effects = self.edit_list(&list, |document| {
            pasted = document.insert_block(block, position, fallback_indent);
            match pasted {
                Some(_) => EditOutcome::Changed,
                None => EditOutcome::Unchanged,
            }
        });
        self.register = register;
        if let Some(task) = pasted {
            self.select_task(task);
        }
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
    fn yank_in_a_list_without_tasks_keeps_the_register_empty() {
        let directory = TempDir::new().unwrap();
        fs::write(directory.path().join("plan.md"), "# Plan\n").unwrap();
        let mut app = app_in(directory.path());
        app.yank_task();

        let system_under_test = app.register.block();

        assert_eq!(system_under_test, None);
    }

    #[test]
    fn paste_without_an_active_list_has_no_effect() {
        let directory = TempDir::new().unwrap();
        let mut app = app_in(directory.path());

        let system_under_test = app.paste(InsertPosition::Below);

        assert_eq!(system_under_test, []);
    }

    #[test]
    fn paste_keeps_the_block_in_the_register() {
        let directory = TempDir::new().unwrap();
        fs::write(directory.path().join("plan.md"), "- [ ] a\n").unwrap();
        let mut app = app_in(directory.path());
        app.yank_task();
        let expected = app.register.clone();
        let _: Vec<Effect> = app.paste(InsertPosition::Below);

        let system_under_test = &app.register;

        assert_eq!(system_under_test, &expected);
    }
}

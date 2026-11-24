//! Raising and lowering the priority of the selected task.

use crate::app::effect::Effect;
use crate::app::state::App;
use crate::domain::priority::{self, Priority, Step};
use crate::markdown::document::{TaskId, TodoDocument};
use crate::markdown::task_line::FieldKey;

impl App {
    /// Raises the priority of the selected task by one level, up to highest.
    pub(super) fn raise_priority(&mut self) -> Vec<Effect> {
        self.step_priority(Step::Up)
    }

    /// Lowers the priority of the selected task by one level, down to lowest.
    pub(super) fn lower_priority(&mut self) -> Vec<Effect> {
        self.step_priority(Step::Down)
    }

    /// Steps the priority of the selected task of the active list in `direction`.
    fn step_priority(&mut self, direction: Step) -> Vec<Effect> {
        let Some(&task) = self.task_outline().and_then(|outline| outline.selected()) else {
            return Vec::new();
        };
        // `edit_list` borrows the whole app mutably, so the path cannot stay borrowed from it.
        let Some(list) = self.lists.active().cloned() else {
            return Vec::new();
        };
        self.edit_list(&list, |document| {
            let stepped = priority::step(current_priority(document, task), direction);
            document
                .set_field(task, FieldKey::Priority, stepped.map(Priority::as_str))
                .expect("every priority value forms a valid priority field")
        })
    }
}

/// Returns the priority of `task`, where a missing or unknown value means normal priority.
fn current_priority(document: &TodoDocument, task: TaskId) -> Option<Priority> {
    let node = document.tasks().find(|node| node.id() == task)?;
    let value = node.task().field(&FieldKey::Priority)?.value();
    // An unknown value stays visible in the row and counts as normal priority when stepping.
    value.parse().ok()
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::TempDir;

    use super::*;
    use crate::app::state::tests::app_in;

    #[test]
    fn stepping_without_an_active_list_has_no_effect() {
        let directory = TempDir::new().unwrap();
        let mut app = app_in(directory.path());

        let system_under_test = [app.raise_priority(), app.lower_priority()];

        assert!(system_under_test.iter().all(Vec::is_empty));
    }

    #[test]
    fn stepping_in_a_list_without_tasks_has_no_effect() {
        let directory = TempDir::new().unwrap();
        fs::write(directory.path().join("plan.md"), "# Plan\n").unwrap();
        let mut app = app_in(directory.path());

        let system_under_test = [app.raise_priority(), app.lower_priority()];

        assert!(system_under_test.iter().all(Vec::is_empty));
    }

    #[test]
    fn current_priority_of_a_task_missing_from_the_document_is_normal() {
        let document = TodoDocument::parse("- [ ] a [priority:: high]\n");
        let other_task = TodoDocument::parse("- [ ] a\n- [ ] b\n")
            .tasks()
            .nth(1)
            .unwrap()
            .id();

        let system_under_test = current_priority(&document, other_task);

        assert_eq!(system_under_test, None);
    }
}

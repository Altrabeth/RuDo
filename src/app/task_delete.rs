//! Deleting the selected task with its subtasks, without asking: undo brings it back.

use crate::app::effect::Effect;
use crate::app::state::App;
use crate::app::status::StatusMessage;
use crate::app::task_visibility::CompletedTasks;
use crate::config::keymap::action::Action;
use crate::config::keymap::keymap::Keymap;
use crate::markdown::document::{EditOutcome, TaskBlock, TaskId, TodoDocument};

impl App {
    /// Deletes the selected task of the active list with its block and reports how many tasks
    /// went with it.
    pub(super) fn delete_task(&mut self) -> Vec<Effect> {
        self.remove_selected_task()
            .map_or_else(Vec::new, |(_removed, effects)| effects)
    }

    /// Removes the selected task with its block, selects its next visible sibling, else its
    /// previous one, else its parent, and returns the removed block with the effects of the
    /// edit; `None` if no task is selected.
    fn remove_selected_task(&mut self) -> Option<(TaskBlock, Vec<Effect>)> {
        let &task = self.task_outline()?.selected()?;
        // `edit_list` borrows the whole app mutably, so the path cannot stay borrowed from it.
        let list = self.lists.active()?.clone();
        let document = self.active_list()?.content().ok()?.document();
        let task_count = document.block(task)?.task_ids().count();
        let successor = successor(document, task, self.completed_tasks);
        let mut removed = None;
        let effects = self.edit_list(&list, |document| {
            removed = document.remove_block(task);
            match removed {
                Some(_) => EditOutcome::Changed,
                None => EditOutcome::Unchanged,
            }
        });
        let removed = removed?;
        match successor {
            Some(successor) => self.select_task(successor),
            None => self.refresh_tasks(),
        }
        self.show_message(StatusMessage::info(deleted_message(
            &self.keymap,
            task_count,
        )));
        Some((removed, effects))
    }
}

/// Returns the task to select after `task` is deleted from `document`: its next sibling shown
/// with `completed_tasks`, else its previous shown sibling, else its parent.
fn successor(
    document: &TodoDocument,
    task: TaskId,
    completed_tasks: CompletedTasks,
) -> Option<TaskId> {
    let parent = document.parent(task);
    let mut siblings = document
        .tasks()
        .filter(|node| node.parent() == parent && completed_tasks.shows(node.task().status()))
        .map(|node| node.id());
    // `take_while` also consumes `task` itself, so `next` continues after it.
    let previous = siblings
        .by_ref()
        .take_while(|&sibling| sibling != task)
        .last();
    siblings.next().or(previous).or(parent)
}

/// Returns `deleted N tasks (<key> to undo)` with the first key bound to `undo`, without the
/// hint if it is unbound.
fn deleted_message(keymap: &Keymap, task_count: usize) -> String {
    let noun = if task_count == 1 { "task" } else { "tasks" };
    match keymap.keys(Action::Undo).first() {
        Some(keys) => format!("deleted {task_count} {noun} ({} to undo)", keys.compact()),
        None => format!("deleted {task_count} {noun}"),
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;
    use crate::config::keymap::keymap::KeyBindings;

    fn task_id(document: &TodoDocument, description: &str) -> TaskId {
        document
            .tasks()
            .find(|node| node.task().description() == description)
            .unwrap()
            .id()
    }

    #[rstest]
    #[case::next_sibling("a", CompletedTasks::Shown, Some("c"))]
    #[case::next_shown_sibling_skips_hidden("a", CompletedTasks::Hidden, Some("d"))]
    #[case::previous_sibling("d", CompletedTasks::Shown, Some("c"))]
    #[case::previous_shown_sibling_skips_hidden("d", CompletedTasks::Hidden, Some("a"))]
    #[case::parent("b", CompletedTasks::Shown, Some("a"))]
    fn successor_prefers_next_then_previous_sibling_then_parent(
        #[case] deleted: &str,
        #[case] completed_tasks: CompletedTasks,
        #[case] expected: Option<&str>,
    ) {
        let document = TodoDocument::parse("- [ ] a\n\t- [ ] b\n- [x] c\n- [ ] d\n");
        let task = task_id(&document, deleted);
        let expected = expected.map(|description| task_id(&document, description));

        let system_under_test = successor(&document, task, completed_tasks);

        assert_eq!(system_under_test, expected);
    }

    #[test]
    fn successor_of_the_only_task_is_none() {
        let document = TodoDocument::parse("- [ ] a\n");
        let task = task_id(&document, "a");

        let system_under_test = successor(&document, task, CompletedTasks::Shown);

        assert_eq!(system_under_test, None);
    }

    #[rstest]
    #[case::one_task("", 1, "deleted 1 task (u to undo)")]
    #[case::several_tasks("", 3, "deleted 3 tasks (u to undo)")]
    #[case::first_of_several_keys("undo: [ctrl-z, u]", 2, "deleted 2 tasks (ctrl-z to undo)")]
    #[case::unbound("undo: []", 1, "deleted 1 task")]
    fn deleted_message_names_the_count_and_the_undo_key(
        #[case] yaml: &str,
        #[case] task_count: usize,
        #[case] expected: &str,
    ) {
        let bindings: KeyBindings = serde_saphyr::from_str(yaml).unwrap();
        let keymap = Keymap::new(&bindings).unwrap();

        let system_under_test = deleted_message(&keymap, task_count);

        assert_eq!(system_under_test, expected);
    }
}

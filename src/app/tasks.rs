//! The state of the tasks pane: navigation in the task tree of the active list.
//!
//! Like the lists pane, [`TasksPane`] never copies the tasks. [`task_items`] lists them from
//! the document whenever the pane needs them, and the [`Outline`] remembers the selection by
//! [`TaskId`], so it stays on its task while edits shift the lines around it.

use std::collections::HashMap;
use std::mem;

use crate::app::lists::EntryChange;
use crate::app::state::{App, Focus};
use crate::app::task_visibility::CompletedTasks;
use crate::config::keymap::action::Action;
use crate::markdown::document::{InsertPosition, TaskId, TodoDocument};
use crate::markdown::task_line::TaskLine;
use crate::storage::workspace::{List, ListPath, Workspace};
use crate::ui::outline::{Outcome, Outline, OutlineCommand, OutlineItem, prune};

/// A task of a document with its id and nesting depth.
#[derive(Debug)]
pub struct TaskItem<'document> {
    id: TaskId,
    depth: usize,
    task: &'document TaskLine,
}

impl<'document> TaskItem<'document> {
    /// Returns the task line of this row.
    #[must_use]
    pub fn task(&self) -> &'document TaskLine {
        self.task
    }
}

impl OutlineItem for TaskItem<'_> {
    type Key = TaskId;

    fn key(&self) -> &TaskId {
        &self.id
    }

    fn depth(&self) -> usize {
        self.depth
    }
}

/// Returns every task of `document` in display order, parents before their subtasks, ready
/// for an [`Outline`].
#[must_use]
pub fn task_items(document: &TodoDocument) -> Vec<TaskItem<'_>> {
    all_task_items(document).collect()
}

/// Returns the tasks of `document` that the tasks pane shows with `completed_tasks`, in
/// display order; a task below a hidden task is hidden with it.
#[must_use]
pub fn visible_task_items(
    document: &TodoDocument,
    completed_tasks: CompletedTasks,
) -> Vec<TaskItem<'_>> {
    prune(all_task_items(document), |item| {
        completed_tasks.shows(item.task.status())
    })
    .collect()
}

fn all_task_items(document: &TodoDocument) -> impl Iterator<Item = TaskItem<'_>> {
    document.tasks().map(|node| TaskItem {
        id: node.id(),
        depth: node.depth(),
        task: node.task(),
    })
}

/// Returns the tasks of the list at `path` that the tasks pane shows with `completed_tasks`;
/// none if it is missing or could not be loaded, which the tasks pane shows instead.
pub(super) fn list_items<'workspace>(
    workspace: &'workspace Workspace,
    path: &ListPath,
    completed_tasks: CompletedTasks,
) -> Vec<TaskItem<'workspace>> {
    match workspace.list(path).map(List::content) {
        Some(Ok(loaded)) => visible_task_items(loaded.document(), completed_tasks),
        Some(Err(_)) | None => Vec::new(),
    }
}

/// The tasks pane: the selection, folding and scrolling of the task tree of every list opened
/// so far, so that each list shows the task it showed when the user left it.
#[derive(Debug, Clone, Default)]
pub struct TasksPane {
    outlines: HashMap<ListPath, Outline<TaskId>>,
}

impl TasksPane {
    /// Returns the navigation state of the task tree of the list at `path`, or `None` if the
    /// list was never opened.
    #[must_use]
    pub fn outline(&self, path: &ListPath) -> Option<&Outline<TaskId>> {
        self.outlines.get(path)
    }

    /// Forgets the selection and folding of the list at `path`, whose task ids are void
    /// because its document was parsed again, and returns its fresh navigation state, or
    /// `None` if the list was never opened.
    pub(super) fn reset(&mut self, path: &ListPath) -> Option<&mut Outline<TaskId>> {
        let outline = self.outlines.get_mut(path)?;
        *outline = Outline::new();
        Some(outline)
    }

    /// Returns the navigation state of the list at `path`, creating it on first use.
    fn outline_mut(&mut self, path: &ListPath) -> &mut Outline<TaskId> {
        // `entry` would clone the path on every call, also for lists opened before.
        if !self.outlines.contains_key(path) {
            self.outlines.insert(path.clone(), Outline::new());
        }
        self.outlines
            .get_mut(path)
            .expect("the outline of the list was inserted above")
    }

    /// Follows `change` of the workspace: the navigation state of a renamed list moves to its
    /// new path and that of a removed list is dropped.
    pub(super) fn apply_entry_change(&mut self, change: EntryChange<'_>) {
        self.outlines = mem::take(&mut self.outlines)
            .into_iter()
            .filter_map(|(path, outline)| change.list_path(path).map(|path| (path, outline)))
            .collect();
    }
}

impl App {
    /// Returns the list whose tasks the tasks pane shows, or `None` if no list is open.
    #[must_use]
    pub fn active_list(&self) -> Option<&List> {
        self.workspace.list(self.lists.active()?)
    }

    /// Returns the navigation state of the task tree of the active list, including the
    /// selected task, or `None` if no list is open.
    #[must_use]
    pub fn task_outline(&self) -> Option<&Outline<TaskId>> {
        self.tasks.outline(self.lists.active()?)
    }

    /// Returns `position` relative to the selected task of the active list, or
    /// [`InsertPosition::End`] without a selected task.
    pub(super) fn insert_position(&self, position: fn(TaskId) -> InsertPosition) -> InsertPosition {
        self.task_outline()
            .and_then(Outline::selected)
            .map_or(InsertPosition::End, |&selected| position(selected))
    }

    /// Brings the task tree of the active list in line with its document and the height of
    /// the pane: after opening the list, an edit or a resize.
    ///
    /// The selection stays on its task; if the task was removed or hidden, the task now in its
    /// row is selected.
    pub(super) fn refresh_tasks(&mut self) {
        let viewport_height = self.viewport_height(Focus::Tasks);
        let Some(path) = self.lists.active() else {
            return;
        };
        let items = list_items(&self.workspace, path, self.completed_tasks);
        self.tasks
            .outline_mut(path)
            .refresh(&items, viewport_height);
    }

    /// Selects `task` in the task tree of the active list and unfolds the tasks that contain
    /// it, so that it stays visible after an edit moved it under a folded task.
    pub(super) fn select_task(&mut self, task: TaskId) {
        let viewport_height = self.viewport_height(Focus::Tasks);
        let Some(path) = self.lists.active() else {
            return;
        };
        let items = list_items(&self.workspace, path, self.completed_tasks);
        match self
            .tasks
            .outline_mut(path)
            .select(&items, &task, viewport_height)
        {
            // Whether the selection moved does not matter: the task is selected either way.
            Outcome::Changed | Outcome::Unchanged => {}
        }
    }

    /// Performs a navigation or folding action in the tasks pane.
    ///
    /// `collapse` on a top-level task that cannot fold, or in a pane without tasks, focuses
    /// the lists pane.
    pub(super) fn update_tasks(&mut self, action: Action) {
        let Some(command) = OutlineCommand::from_action(action) else {
            return;
        };
        let viewport_height = self.viewport_height(Focus::Tasks);
        let outcome = match self.lists.active() {
            Some(path) => {
                let items = list_items(&self.workspace, path, self.completed_tasks);
                self.tasks
                    .outline_mut(path)
                    .apply(&items, command, viewport_height)
            }
            None => Outcome::Unchanged,
        };
        if command == OutlineCommand::Collapse && outcome == Outcome::Unchanged {
            self.focus = Focus::Lists;
        }
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::TempDir;

    use super::*;
    use crate::app::state::tests::{app_in, only_list_path};

    #[test]
    fn task_items_list_tasks_with_their_depths() {
        let document = TodoDocument::parse("# Plan\n- [ ] a\n\t- [x] b\n\t\t- [ ] c\n- [ ] d\n");
        let expected = [("a", 0), ("b", 1), ("c", 2), ("d", 0)];

        let system_under_test: Vec<(&str, usize)> = task_items(&document)
            .iter()
            .map(|item| (item.task().description(), item.depth()))
            .collect();

        assert_eq!(system_under_test, expected);
    }

    #[test]
    fn task_items_are_keyed_by_task_id() {
        let document = TodoDocument::parse("- [ ] a\n- [ ] b\n");
        let expected: Vec<TaskId> = document.tasks().map(|node| node.id()).collect();

        let system_under_test: Vec<TaskId> = task_items(&document)
            .iter()
            .map(|item| *item.key())
            .collect();

        assert_eq!(system_under_test, expected);
    }

    #[test]
    fn unreadable_list_has_no_items() {
        let directory = TempDir::new().unwrap();
        fs::write(directory.path().join("binary.md"), [0xff, 0xfe]).unwrap();
        let app = app_in(directory.path());
        let path = only_list_path(&app);

        let system_under_test = list_items(app.workspace(), &path, CompletedTasks::Shown);

        assert!(system_under_test.is_empty());
    }

    #[test]
    fn outline_is_created_once_per_list() {
        let directory = TempDir::new().unwrap();
        fs::write(directory.path().join("plan.md"), "- [ ] a\n").unwrap();
        let app = app_in(directory.path());
        let path = only_list_path(&app);
        let mut pane = TasksPane::default();
        pane.outline_mut(&path);

        let system_under_test = pane.outline_mut(&path);

        assert_eq!(system_under_test.selected(), None);
        assert_eq!(pane.outlines.len(), 1);
    }

    #[test]
    fn navigation_without_an_active_list_keeps_the_focus() {
        let directory = TempDir::new().unwrap();
        let mut app = app_in(directory.path());
        app.focus = Focus::Tasks;
        app.update_tasks(Action::MoveDown);

        let system_under_test = app.task_outline();

        assert!(system_under_test.is_none());
        assert_eq!(app.focus(), Focus::Tasks);
    }

    #[test]
    fn actions_without_an_outline_command_are_ignored() {
        let directory = TempDir::new().unwrap();
        let mut app = app_in(directory.path());
        app.focus = Focus::Tasks;
        app.update_tasks(Action::Open);

        let system_under_test = app.focus();

        assert_eq!(system_under_test, Focus::Tasks);
    }
}

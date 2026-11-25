//! The state of the tasks pane: navigation in the task tree of the active list.
//!
//! Like the lists pane, [`TasksPane`] never copies the tasks. [`task_items`] lists them from
//! the document whenever the pane needs them, and the [`Outline`] remembers the selection by
//! [`TaskId`], so it stays on its task while edits shift the lines around it. Every list has
//! its own search filter, because task ids only identify tasks within their document.

use std::collections::HashMap;
use std::mem;

use crate::app::lists::EntryChange;
use crate::app::search::{PaneFilter, SearchItem};
use crate::app::state::{App, Focus};
use crate::app::task_visibility::CompletedTasks;
use crate::config::keymap::action::Action;
use crate::markdown::document::{InsertPosition, TaskId, TodoDocument};
use crate::markdown::task_line::TaskLine;
use crate::storage::workspace::{List, ListPath, Workspace};
use crate::ui::outline::{FilterRole, Outcome, Outline, OutlineCommand, OutlineItem, prune};

/// A task of a document with its id, nesting depth and why the search filter shows it.
#[derive(Debug)]
pub struct TaskItem<'document> {
    id: TaskId,
    depth: usize,
    task: &'document TaskLine,
    role: FilterRole,
}

impl<'document> TaskItem<'document> {
    /// Returns the task line of this row.
    #[must_use]
    pub fn task(&self) -> &'document TaskLine {
        self.task
    }

    /// Returns whether the task matches the search filter or only contains a match.
    #[must_use]
    pub fn role(&self) -> FilterRole {
        self.role
    }
}

impl SearchItem for TaskItem<'_> {
    fn search_text(&self) -> Option<&str> {
        Some(self.task.description())
    }

    fn with_role(self, role: FilterRole) -> Self {
        Self { role, ..self }
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

/// Returns the tasks of `document` that the tasks pane shows with `completed_tasks` and
/// `filter`, in display order.
///
/// A task below a hidden completed task is hidden with it, also if it matches the filter;
/// the ancestors of a match are shown as [`FilterRole::Context`].
#[must_use]
pub fn visible_task_items<'document>(
    document: &'document TodoDocument,
    completed_tasks: CompletedTasks,
    filter: Option<&PaneFilter<TaskId>>,
) -> Vec<TaskItem<'document>> {
    filtered(unfiltered_task_items(document, completed_tasks), filter)
}

/// Returns the tasks of `document` that `completed_tasks` shows, before any search filter.
fn unfiltered_task_items(
    document: &TodoDocument,
    completed_tasks: CompletedTasks,
) -> impl Iterator<Item = TaskItem<'_>> {
    prune(all_task_items(document), move |item| {
        completed_tasks.shows(item.task.status())
    })
}

fn all_task_items(document: &TodoDocument) -> impl Iterator<Item = TaskItem<'_>> {
    document.tasks().map(|node| TaskItem {
        id: node.id(),
        depth: node.depth(),
        task: node.task(),
        role: FilterRole::Match,
    })
}

/// Returns the tasks of `items` that `filter` keeps, or all of them without a filter.
fn filtered<'document>(
    items: impl Iterator<Item = TaskItem<'document>>,
    filter: Option<&PaneFilter<TaskId>>,
) -> Vec<TaskItem<'document>> {
    match filter {
        Some(filter) => filter.apply(items),
        None => items.collect(),
    }
}

/// Returns the tasks of the list at `path` that `completed_tasks` shows, before any search
/// filter; none if it is missing or could not be loaded, which the tasks pane shows instead.
pub(super) fn unfiltered_list_items<'workspace>(
    workspace: &'workspace Workspace,
    path: &ListPath,
    completed_tasks: CompletedTasks,
) -> impl Iterator<Item = TaskItem<'workspace>> {
    workspace
        .list(path)
        .map(List::content)
        .and_then(Result::ok)
        .into_iter()
        .flat_map(move |loaded| unfiltered_task_items(loaded.document(), completed_tasks))
}

/// Returns the tasks of the list at `path` that the tasks pane shows with `completed_tasks`
/// and `filter`; none if it is missing or could not be loaded.
pub(super) fn list_items<'workspace>(
    workspace: &'workspace Workspace,
    path: &ListPath,
    completed_tasks: CompletedTasks,
    filter: Option<&PaneFilter<TaskId>>,
) -> Vec<TaskItem<'workspace>> {
    filtered(
        unfiltered_list_items(workspace, path, completed_tasks),
        filter,
    )
}

/// The navigation state and the search filter of the task tree of one list.
#[derive(Debug, Clone, Default)]
struct TaskTree {
    outline: Outline<TaskId>,
    filter: PaneFilter<TaskId>,
}

/// The tasks pane: the selection, folding, scrolling and search filter of the task tree of
/// every list opened so far, so that each list shows the task it showed when the user left it.
#[derive(Debug, Clone, Default)]
pub struct TasksPane {
    trees: HashMap<ListPath, TaskTree>,
}

impl TasksPane {
    /// Returns the navigation state of the task tree of the list at `path`, or `None` if the
    /// list was never opened.
    #[must_use]
    pub fn outline(&self, path: &ListPath) -> Option<&Outline<TaskId>> {
        self.trees.get(path).map(|tree| &tree.outline)
    }

    /// Returns the search filter of the list at `path`, or `None` if the list was never
    /// opened.
    #[must_use]
    pub fn filter(&self, path: &ListPath) -> Option<&PaneFilter<TaskId>> {
        self.trees.get(path).map(|tree| &tree.filter)
    }

    /// Forgets the selection, folding and revealed tasks of the list at `path`, whose task ids
    /// are void because its document was parsed again, and tells whether the list was ever
    /// opened. The search query is kept.
    #[must_use]
    pub(super) fn reset(&mut self, path: &ListPath) -> bool {
        let Some(tree) = self.trees.get_mut(path) else {
            return false;
        };
        tree.outline = Outline::new();
        tree.outline.set_folding(tree.filter.folding());
        tree.filter.forget_revealed();
        true
    }

    /// Returns the search filter of the list at `path`, creating it on first use.
    pub(super) fn filter_mut(&mut self, path: &ListPath) -> &mut PaneFilter<TaskId> {
        &mut self.tree_mut(path).filter
    }

    /// Returns the lists that were opened so far.
    pub(super) fn lists(&self) -> impl Iterator<Item = &ListPath> {
        self.trees.keys()
    }

    /// Forgets the navigation state of the list at `path`, which is no longer in the
    /// workspace.
    pub(super) fn forget(&mut self, path: &ListPath) {
        self.trees.remove(path);
    }

    /// Returns the navigation state of the list at `path`, creating it on first use.
    pub(super) fn outline_mut(&mut self, path: &ListPath) -> &mut Outline<TaskId> {
        &mut self.tree_mut(path).outline
    }

    /// Returns the task tree of the list at `path`, creating it on first use.
    fn tree_mut(&mut self, path: &ListPath) -> &mut TaskTree {
        // `entry` would clone the path on every call, also for lists opened before.
        if !self.trees.contains_key(path) {
            self.trees.insert(path.clone(), TaskTree::default());
        }
        self.trees
            .get_mut(path)
            .expect("the task tree of the list was inserted above")
    }

    /// Follows `change` of the workspace: the navigation state and filter of a renamed list
    /// move to its new path and those of a removed list are dropped.
    pub(super) fn apply_entry_change(&mut self, change: EntryChange<'_>) {
        self.trees = mem::take(&mut self.trees)
            .into_iter()
            .filter_map(|(path, tree)| change.list_path(path).map(|path| (path, tree)))
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

    /// Returns the search filter of the active list, or `None` if no list is open.
    #[must_use]
    pub fn task_filter(&self) -> Option<&PaneFilter<TaskId>> {
        self.tasks.filter(self.lists.active()?)
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
        let items = list_items(
            &self.workspace,
            path,
            self.completed_tasks,
            self.tasks.filter(path),
        );
        self.tasks
            .outline_mut(path)
            .refresh(&items, viewport_height);
    }

    /// Filters the task tree of the active list again after its filter changed.
    pub(super) fn refilter_tasks(&mut self) {
        let viewport_height = self.viewport_height(Focus::Tasks);
        let Some(path) = self.lists.active() else {
            return;
        };
        let completed_tasks = self.completed_tasks;
        let workspace = &self.workspace;
        let tree = self.tasks.tree_mut(path);
        tree.filter.refilter(
            &mut tree.outline,
            || unfiltered_list_items(workspace, path, completed_tasks),
            viewport_height,
        );
    }

    /// Selects `task` in the task tree of the active list, unfolds the tasks that contain it
    /// and reveals it while a filter is active, so that it stays visible after an edit moved it
    /// under a folded task or changed it so that it no longer matches.
    pub(super) fn select_task(&mut self, task: TaskId) {
        let viewport_height = self.viewport_height(Focus::Tasks);
        let Some(path) = self.lists.active() else {
            return;
        };
        self.tasks.filter_mut(path).reveal(task);
        let items = list_items(
            &self.workspace,
            path,
            self.completed_tasks,
            self.tasks.filter(path),
        );
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
                let items = list_items(
                    &self.workspace,
                    path,
                    self.completed_tasks,
                    self.tasks.filter(path),
                );
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

        let system_under_test = list_items(app.workspace(), &path, CompletedTasks::Shown, None);

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
        assert_eq!(pane.trees.len(), 1);
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

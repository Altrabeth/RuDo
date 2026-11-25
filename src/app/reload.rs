//! Reloading a list whose file another program changed, such as an external editor.

use crate::app::state::{App, Focus};
use crate::app::tasks::list_items;
use crate::storage::workspace::{List, ListPath, Reload};
use crate::ui::outline::Outcome;

impl App {
    /// Reads the list at `list` from disk again and replaces its document if the file changed
    /// since RuDo last loaded or saved it, judged by its fingerprint.
    ///
    /// This is how changes made by other programs win over the in-memory state, both after
    /// the external editor exits and when the file watcher reports a change. Re-parsing
    /// assigns new task ids, so the tasks pane of the list starts afresh: folds are opened
    /// and the selection moves to the task on the line of the previously selected task, or
    /// to the last task before that line. A file that cannot be loaded any more makes the
    /// list show its load error. The undo history of a replaced list is forgotten, because it
    /// describes a document that no longer exists. An unchanged file and a list missing from
    /// the workspace are left alone.
    pub fn reload_list(&mut self, list: &ListPath) {
        let selected_line = self.selected_line(list);
        match self.workspace.reload(list) {
            Some(Reload::Replaced) => {
                tracing::info!(%list, "list changed on disk and was reloaded");
                self.history.forget(list);
                self.restore_selection(list, selected_line);
            }
            Some(Reload::Unchanged) => {}
            None => tracing::warn!(%list, "list to reload is not in the workspace"),
        }
    }

    /// Returns the 1-based line number of the task selected in the tasks pane of `list`.
    fn selected_line(&self, list: &ListPath) -> Option<usize> {
        let task = *self.tasks.outline(list)?.selected()?;
        let loaded = self.workspace.list(list).map(List::content)?.ok()?;
        loaded.document().line_number(task)
    }

    /// Selects the task on `selected_line`, or the last one before it, in the tasks pane of
    /// `list` after its document was replaced; the first visible task if that task is hidden.
    fn restore_selection(&mut self, list: &ListPath, selected_line: Option<usize>) {
        let viewport_height = self.viewport_height(Focus::Tasks);
        let Some(outline) = self.tasks.reset(list) else {
            return;
        };
        let items = list_items(&self.workspace, list, self.completed_tasks);
        let task = self
            .workspace
            .list(list)
            .map(List::content)
            .and_then(Result::ok)
            .zip(selected_line)
            .and_then(|(loaded, line)| loaded.document().task_at_or_before(line));
        match task {
            // The outline was reset, so selecting fails only for a hidden completed task.
            Some(task) if outline.select(&items, &task, viewport_height) == Outcome::Changed => {}
            Some(_) | None => outline.refresh(&items, viewport_height),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::TempDir;

    use super::*;
    use crate::app::state::tests::{app_in, only_list_path};
    use crate::config::keymap::action::Action;
    use crate::markdown::document::TaskId;
    use crate::storage::workspace::Entry;

    /// Returns the description of the task selected in the tasks pane of the active list.
    fn selected_description(app: &App) -> Option<String> {
        let task: TaskId = *app.task_outline()?.selected()?;
        let loaded = app.active_list()?.content().ok()?;
        let node = loaded.document().tasks().find(|node| node.id() == task)?;
        Some(node.task().description().to_owned())
    }

    #[test]
    fn reload_selects_the_task_on_the_line_of_the_selected_task() {
        let directory = TempDir::new().unwrap();
        let file = directory.path().join("plan.md");
        fs::write(&file, "- [ ] a\n- [ ] b\n- [ ] c\n").unwrap();
        let mut app = app_in(directory.path());
        let list = only_list_path(&app);
        app.update_tasks(Action::MoveDown);
        fs::write(&file, "- [ ] new\n- [ ] a\n- [ ] b\n- [ ] c\n").unwrap();
        app.reload_list(&list);

        let system_under_test = selected_description(&app);

        assert_eq!(system_under_test.as_deref(), Some("a"));
    }

    #[test]
    fn reload_selects_the_last_task_before_a_line_that_has_none() {
        let directory = TempDir::new().unwrap();
        let file = directory.path().join("plan.md");
        fs::write(&file, "- [ ] a\n- [ ] b\n- [ ] c\n").unwrap();
        let mut app = app_in(directory.path());
        let list = only_list_path(&app);
        app.update_tasks(Action::MoveToBottom);
        fs::write(&file, "- [ ] a\n").unwrap();
        app.reload_list(&list);

        let system_under_test = selected_description(&app);

        assert_eq!(system_under_test.as_deref(), Some("a"));
    }

    #[test]
    fn reload_of_a_list_that_cannot_be_loaded_any_more_shows_no_tasks() {
        let directory = TempDir::new().unwrap();
        let file = directory.path().join("plan.md");
        fs::write(&file, "- [ ] a\n").unwrap();
        let mut app = app_in(directory.path());
        let list = only_list_path(&app);
        fs::write(&file, [0xff]).unwrap();
        app.reload_list(&list);

        let system_under_test = app.task_outline().unwrap().selected();

        assert_eq!(system_under_test, None);
        assert!(app.active_list().unwrap().content().is_err());
    }

    #[test]
    fn reload_of_a_list_that_was_never_opened_only_replaces_the_document() {
        let directory = TempDir::new().unwrap();
        fs::write(directory.path().join("a.md"), "").unwrap();
        let file = directory.path().join("b.md");
        fs::write(&file, "- [ ] old\n").unwrap();
        let mut app = app_in(directory.path());
        let list = app
            .workspace()
            .entries()
            .iter()
            .find_map(|entry| match entry {
                Entry::List(list) if list.name() == "b" => Some(list.path().clone()),
                Entry::List(_) | Entry::Folder(_) => None,
            })
            .unwrap();
        fs::write(&file, "- [ ] new\n").unwrap();
        app.reload_list(&list);

        let system_under_test = app.workspace().list(&list).unwrap().content().unwrap();

        assert_eq!(system_under_test.document().to_string(), "- [ ] new\n");
        assert!(app.tasks.outline(&list).is_none());
    }

    #[test]
    fn reload_of_a_list_missing_from_the_workspace_changes_nothing() {
        let other_directory = TempDir::new().unwrap();
        fs::write(other_directory.path().join("other.md"), "").unwrap();
        let list = only_list_path(&app_in(other_directory.path()));
        let directory = TempDir::new().unwrap();
        let mut app = app_in(directory.path());
        app.reload_list(&list);

        let system_under_test = app.workspace().entries();

        assert!(system_under_test.is_empty());
    }
}

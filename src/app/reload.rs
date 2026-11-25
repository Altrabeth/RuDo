//! Following changes that other programs, such as an external editor, Obsidian or `git`,
//! make to the workspace: deciding what a reported change means with [`App::on_fs_changed`],
//! reloading a changed list and scanning the workspace again.

use std::path::Path;

use crate::app::effect::{Effect, error_report};
use crate::app::lists::EntryChange;
use crate::app::state::{App, Focus};
use crate::app::status::StatusMessage;
use crate::app::tasks::list_items;
use crate::storage::watch::{FsChange, PathState, WatchError};
use crate::storage::workspace::{Entry, List, ListPath, Reload, Workspace, has_list_extension};
use crate::ui::outline::Outcome;

/// What a single [`FsChange`] asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Reaction<'workspace> {
    /// The content of this known list changed.
    Reload(&'workspace ListPath),
    /// The tree of folders and lists changed.
    Rescan,
}

impl App {
    /// Decides what the changes reported by the file watcher mean for the workspace, without
    /// touching the disk.
    ///
    /// A file at the path of a known list asks to reload it ([`Effect::ReloadList`]); this
    /// covers editors that save by renaming a temporary file over the list. Anything else
    /// that changes the tree asks for one [`Effect::Rescan`]: a new `.md` file, a new directory
    /// in a folder of the tree, a removed list or folder, or a list and folder replacing each
    /// other. Hidden paths count like any other, because the workspace holds hidden entries
    /// whether the lists pane shows them or not. The bin, files without the `.md` extension
    /// (including RuDo's own temporary files), directories in folders the tree left out (such
    /// as `.git/objects/ab`), known folders that still exist and missing unknown paths are
    /// ignored.
    /// RuDo's own saves and entry operations are reported too; they reload nothing, because
    /// the workspace already matches the disk and [`App::reload_list`] compares fingerprints.
    ///
    /// Each list is reloaded at most once, in path order, before the rescan.
    #[must_use]
    pub fn on_fs_changed(&self, changes: &[FsChange]) -> Vec<Effect> {
        let mut lists = Vec::new();
        let mut rescan = false;
        for change in changes {
            match reaction(&self.workspace, change) {
                Some(Reaction::Reload(list)) => lists.push(list),
                Some(Reaction::Rescan) => rescan = true,
                None => {}
            }
        }
        lists.sort_unstable();
        lists.dedup();
        lists
            .into_iter()
            .cloned()
            .map(Effect::ReloadList)
            .chain(rescan.then_some(Effect::Rescan))
            .collect()
    }

    /// Scans the workspace folder again after another program created, removed or renamed
    /// lists or folders, and brings both panes in line with the new tree.
    ///
    /// Lists whose file did not change keep their document, so their selection, folds and
    /// undo history stay as they are; a list whose file changed is reloaded as
    /// [`App::reload_list`] does. The lists pane keeps its selection and folds where the
    /// entries still exist. The state of removed lists is forgotten, and if the active list
    /// was removed, the tasks pane empties and a message says so. A workspace folder that
    /// cannot be read any more keeps the current tree and shows an error message.
    pub fn rescan(&mut self) {
        let selected_lines: Vec<(&ListPath, usize)> = self
            .tasks
            .lists()
            .filter_map(|list| Some((list, self.selected_line(list)?)))
            .collect();
        let rescan = match self.workspace.rescan() {
            Ok(rescan) => rescan,
            Err(error) => {
                let report = error_report(&error);
                tracing::warn!(%report, "cannot scan the workspace again");
                self.show_message(StatusMessage::error(report));
                return;
            }
        };
        tracing::info!("workspace changed on disk and was scanned again");
        let replaced: Vec<(&ListPath, Option<usize>)> = rescan
            .replaced()
            .iter()
            .map(|list| {
                let line = selected_lines
                    .iter()
                    .find_map(|&(selected, line)| (selected == list).then_some(line));
                (list, line)
            })
            .collect();
        for (list, selected_line) in replaced {
            self.list_replaced(list, selected_line);
        }
        for list in rescan.removed() {
            self.list_removed(list);
        }
        self.lists
            .refresh(&self.workspace, self.viewport_height(Focus::Lists));
        self.refresh_tasks();
        self.end_visual_mode_without_anchor();
    }

    /// Forgets the state of `list`, which is no longer in the workspace, and closes it if it
    /// is the active list.
    fn list_removed(&mut self, list: &List) {
        let path = list.path();
        self.history.forget(path);
        self.tasks.forget(path);
        if self.lists.active() == Some(path) {
            self.apply_entry_change(EntryChange::Removed(path.as_path()));
            self.show_message(StatusMessage::info(format!(
                "{} was removed on disk",
                list.name()
            )));
        }
    }

    /// Reports that the workspace cannot be watched, so changes by other programs only show
    /// up after the external editor exits or RuDo restarts; RuDo keeps running.
    pub fn watch_failed(&mut self, error: &WatchError) {
        let report = error_report(error);
        tracing::warn!(%report, "live reload disabled");
        self.show_message(StatusMessage::error(format!(
            "live reload disabled: {report}"
        )));
    }

    /// Reads the list at `list` from disk again and replaces its document if the file changed
    /// since RuDo last loaded or saved it, judged by its fingerprint.
    ///
    /// This is how changes made by other programs win over the in-memory state, both after
    /// the external editor exits and when the file watcher reports a change. The new
    /// document gets task ids the old one never used, so the tasks pane of the list starts
    /// afresh: folds are opened and the selection moves to the task on the line of the
    /// previously selected task, or to the last task before that line; an open inline editor
    /// or prompt for a task of the old document finds its task gone. A file that cannot be
    /// loaded any more makes the list show its load error. The undo history of a replaced
    /// list is forgotten, because it describes a document that no longer exists, and the
    /// message `<list> reloaded` is shown. An unchanged file and a list missing from the
    /// workspace are left alone.
    pub fn reload_list(&mut self, list: &ListPath) {
        let selected_line = self.selected_line(list);
        match self.workspace.reload(list) {
            Some(Reload::Replaced) => {
                tracing::info!(%list, "list changed on disk and was reloaded");
                self.list_replaced(list, selected_line);
                let message = format!("{} reloaded", self.list_name(list));
                self.show_message(StatusMessage::info(message));
                self.end_visual_mode_without_anchor();
            }
            Some(Reload::Unchanged) => {}
            None => tracing::warn!(%list, "list to reload is not in the workspace"),
        }
    }

    /// Follows a save of `list` that was refused because another program changed its file
    /// since RuDo loaded it: the file on disk wins, so the list is reloaded and the message
    /// says that the last change was dropped.
    pub(super) fn save_conflicted(&mut self, list: &ListPath) {
        tracing::warn!(%list, "list changed on disk before it was saved; the change was dropped");
        self.reload_list(list);
        let message = format!(
            "{} changed on disk; your last change was not applied",
            self.list_name(list)
        );
        self.show_message(StatusMessage::error(message));
    }

    /// Follows the replaced document of `list`: forgets its undo history, which describes a
    /// document that no longer exists, and selects the task on `selected_line`.
    fn list_replaced(&mut self, list: &ListPath, selected_line: Option<usize>) {
        self.history.forget(list);
        self.restore_selection(list, selected_line);
    }

    /// Returns the name of the list at `list`, or an empty name if it is not in the
    /// workspace.
    fn list_name(&self, list: &ListPath) -> &str {
        self.workspace.list(list).map_or("", List::name)
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
        if !self.tasks.reset(list) {
            return;
        }
        let items = list_items(
            &self.workspace,
            list,
            self.completed_tasks,
            self.tasks.filter(list),
        );
        let outline = self.tasks.outline_mut(list);
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

/// Returns what `change` asks for in `workspace`, or `None` if it does not matter.
fn reaction<'workspace>(
    workspace: &'workspace Workspace,
    change: &FsChange,
) -> Option<Reaction<'workspace>> {
    let path = change.path();
    if workspace.bin().contains(path) {
        return None;
    }
    match (change.state(), workspace.entry(path)) {
        (PathState::File, Some(Entry::List(list))) => Some(Reaction::Reload(list.path())),
        (PathState::File, None) => has_list_extension(path).then_some(Reaction::Rescan),
        (PathState::Directory, None) => is_in_tree(workspace, path).then_some(Reaction::Rescan),
        (PathState::Directory, Some(Entry::Folder(_))) | (PathState::Missing, None) => None,
        (PathState::File, Some(Entry::Folder(_)))
        | (PathState::Directory, Some(Entry::List(_)))
        | (PathState::Missing, Some(_)) => Some(Reaction::Rescan),
    }
}

/// Returns whether a new directory at `path` belongs in the tree of `workspace`: the
/// workspace folder itself, or a directory in a folder of the tree.
///
/// A directory in a folder the tree left out, such as `.git/objects/ab`, changes nothing,
/// because hidden folders without lists are not part of the tree.
fn is_in_tree(workspace: &Workspace, path: &Path) -> bool {
    path.parent()
        .is_none_or(|parent| workspace.contains_folder(parent))
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::Path;

    use rstest::rstest;
    use tempfile::TempDir;

    use super::*;
    use crate::app::effect::execute;
    use crate::app::history::History;
    use crate::app::state::tests::{app_in, only_list_path, toggle_first_task};
    use crate::config::keymap::action::Action;
    use crate::markdown::document::TaskId;
    use crate::storage::watch::WorkspaceWatcher;

    /// What a change is expected to ask for in the workspace of [`classification_app`].
    #[derive(Debug, Clone, Copy)]
    enum Expected {
        Nothing,
        Reload(&'static str),
        Rescan,
    }

    /// Starts an app on a workspace with `inbox.md`, `home/chores.md` and the hidden
    /// `.notes/ideas.md`.
    fn classification_app(root: &Path) -> App {
        fs::create_dir(root.join("home")).unwrap();
        fs::write(root.join("home/chores.md"), "").unwrap();
        fs::create_dir(root.join(".notes")).unwrap();
        fs::write(root.join(".notes/ideas.md"), "").unwrap();
        fs::write(root.join("inbox.md"), "").unwrap();
        app_in(root)
    }

    /// Returns the path of the list at `relative` in the workspace of `app`.
    fn list_path(app: &App, relative: &str) -> ListPath {
        match app.workspace().entry(Path::new(relative)) {
            Some(Entry::List(list)) => list.path().clone(),
            entry => panic!("expected a list at {relative}, got {entry:?}"),
        }
    }

    #[rstest]
    #[case::written_list("inbox.md", PathState::File, Expected::Reload("inbox.md"))]
    #[case::nested_list("home/chores.md", PathState::File, Expected::Reload("home/chores.md"))]
    #[case::new_list("new.md", PathState::File, Expected::Rescan)]
    #[case::new_list_in_capitals("home/NEW.MD", PathState::File, Expected::Rescan)]
    #[case::removed_list("inbox.md", PathState::Missing, Expected::Rescan)]
    #[case::removed_folder("home", PathState::Missing, Expected::Rescan)]
    #[case::new_folder("projects", PathState::Directory, Expected::Rescan)]
    #[case::changed_folder("home", PathState::Directory, Expected::Nothing)]
    #[case::folder_replaced_list("inbox.md", PathState::Directory, Expected::Rescan)]
    #[case::file_replaced_folder("home", PathState::File, Expected::Rescan)]
    #[case::workspace_folder("", PathState::Directory, Expected::Rescan)]
    #[case::other_file("notes.txt", PathState::File, Expected::Nothing)]
    #[case::file_without_extension("home/README", PathState::File, Expected::Nothing)]
    #[case::removed_unknown_path("gone.md", PathState::Missing, Expected::Nothing)]
    #[case::temporary_file(".tmpA1b2C3", PathState::File, Expected::Nothing)]
    #[case::new_hidden_list(".draft.md", PathState::File, Expected::Rescan)]
    #[case::new_hidden_folder(".obsidian", PathState::Directory, Expected::Rescan)]
    #[case::new_list_in_new_hidden_folder(".git/inbox.md", PathState::File, Expected::Rescan)]
    #[case::hidden_folder_inside("home/.trash", PathState::Directory, Expected::Rescan)]
    #[case::written_hidden_list(
        ".notes/ideas.md",
        PathState::File,
        Expected::Reload(".notes/ideas.md")
    )]
    #[case::folder_in_unknown_folder(".git/objects/ab", PathState::Directory, Expected::Nothing)]
    #[case::hidden_file(".obsidian/workspace.json", PathState::File, Expected::Nothing)]
    #[case::bin(".trash", PathState::Directory, Expected::Nothing)]
    #[case::list_in_bin(".trash/inbox.md", PathState::File, Expected::Nothing)]
    #[case::removed_from_bin(".trash/inbox.md", PathState::Missing, Expected::Nothing)]
    fn fs_changes_are_classified(
        #[case] path: &str,
        #[case] state: PathState,
        #[case] expected: Expected,
    ) {
        let directory = TempDir::new().unwrap();
        let app = classification_app(directory.path());
        let changes = [FsChange::new(path, state)];
        let expected = match expected {
            Expected::Nothing => Vec::new(),
            Expected::Reload(list) => vec![Effect::ReloadList(list_path(&app, list))],
            Expected::Rescan => vec![Effect::Rescan],
        };

        let system_under_test = app.on_fs_changed(&changes);

        assert_eq!(system_under_test, expected);
    }

    #[test]
    fn fs_changes_reload_each_list_once_in_path_order_before_one_rescan() {
        let directory = TempDir::new().unwrap();
        let app = classification_app(directory.path());
        let changes = [
            FsChange::new("new.md", PathState::File),
            FsChange::new("inbox.md", PathState::File),
            FsChange::new("projects", PathState::Directory),
            FsChange::new("home/chores.md", PathState::File),
            FsChange::new("inbox.md", PathState::File),
        ];
        let expected = vec![
            Effect::ReloadList(list_path(&app, "home/chores.md")),
            Effect::ReloadList(list_path(&app, "inbox.md")),
            Effect::Rescan,
        ];

        let system_under_test = app.on_fs_changed(&changes);

        assert_eq!(system_under_test, expected);
    }

    #[test]
    fn reload_after_rudos_own_save_keeps_the_document() {
        let directory = TempDir::new().unwrap();
        fs::write(directory.path().join("plan.md"), "- [ ] a\n- [ ] b\n").unwrap();
        let mut app = app_in(directory.path());
        let list = only_list_path(&app);
        app.update_tasks(Action::MoveDown);
        for effect in app.edit_list(&list, toggle_first_task) {
            execute(&mut app, effect);
        }
        let expected_selection = app.task_outline().unwrap().selected().copied();
        execute(&mut app, Effect::ReloadList(list));

        let system_under_test = app.task_outline().unwrap().selected().copied();

        assert_eq!(system_under_test, expected_selection);
        assert_eq!(selected_description(&app).as_deref(), Some("b"));
    }

    #[test]
    fn rescan_adds_new_entries_and_keeps_the_active_list() {
        let directory = TempDir::new().unwrap();
        fs::write(directory.path().join("plan.md"), "- [ ] a\n").unwrap();
        let mut app = app_in(directory.path());
        let list = only_list_path(&app);
        fs::create_dir(directory.path().join("home")).unwrap();
        fs::write(directory.path().join("home/chores.md"), "- [ ] sweep\n").unwrap();
        execute(&mut app, Effect::Rescan);

        let system_under_test = app.lists().active();

        assert_eq!(system_under_test, Some(&list));
        assert!(app.workspace().entry(Path::new("home/chores.md")).is_some());
        assert_eq!(selected_description(&app).as_deref(), Some("a"));
    }

    #[test]
    fn rescan_closes_an_active_list_that_was_removed() {
        let directory = TempDir::new().unwrap();
        let file = directory.path().join("plan.md");
        fs::write(&file, "- [ ] a\n").unwrap();
        let mut app = app_in(directory.path());
        fs::remove_file(&file).unwrap();
        app.rescan();

        let system_under_test = app.lists().active();

        assert_eq!(system_under_test, None);
        assert!(app.workspace().entries().is_empty());
    }

    #[test]
    fn rescan_keeps_the_undo_history_of_an_unchanged_list() {
        let directory = TempDir::new().unwrap();
        fs::write(directory.path().join("plan.md"), "- [ ] a\n").unwrap();
        let mut app = app_in(directory.path());
        let list = only_list_path(&app);
        for effect in app.edit_list(&list, toggle_first_task) {
            execute(&mut app, effect);
        }
        let expected = app.history.clone();
        app.rescan();

        let system_under_test = &app.history;

        assert_eq!(system_under_test, &expected);
        assert_ne!(system_under_test, &History::default());
    }

    #[test]
    fn rescan_of_a_removed_workspace_folder_keeps_the_tree_and_shows_an_error() {
        let directory = TempDir::new().unwrap();
        let root = directory.path().join("todo");
        fs::create_dir(&root).unwrap();
        fs::write(root.join("plan.md"), "").unwrap();
        let mut app = app_in(&root);
        fs::remove_dir_all(&root).unwrap();
        let expected_prefix = format!("cannot read workspace folder {}: ", root.display());
        app.rescan();

        let system_under_test = app.status_message().unwrap();

        assert!(system_under_test.text().starts_with(&expected_prefix));
        assert_eq!(app.workspace().entries().len(), 1);
    }

    #[test]
    fn watch_failure_disables_live_reload_with_a_message() {
        let directory = TempDir::new().unwrap();
        let missing = directory.path().join("missing");
        let error = WorkspaceWatcher::start(&missing, |_| {}).unwrap_err();
        let expected_prefix = format!("live reload disabled: cannot watch {}: ", missing.display());
        let mut app = app_in(directory.path());
        app.watch_failed(&error);

        let system_under_test = app.status_message().unwrap();

        assert!(system_under_test.text().starts_with(&expected_prefix));
    }

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

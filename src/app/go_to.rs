//! Go to anything ([`Mode::GoTo`]): a fuzzy finder over every folder, list and task of the
//! workspace that jumps to the chosen one, like the "go to file" of an editor.
//!
//! The candidates are collected once when the finder opens: the folders and lists the lists
//! pane can show (hidden ones only while they are shown, never the bin), followed by the open
//! and then the completed tasks of every readable list among them. Equal scores keep this
//! order, so ties prefer folders and lists over tasks and open tasks over completed ones.
//! Tasks match by their text, folders and lists by their path with matches in the name first.
//! The typed text is a [`Query`]: with at least one term, only the tasks that match the terms
//! are offered, ranked by the text words.
//!
//! The target is looked up again on `enter`, because a reload may have removed it or, for a
//! task, given it a new [`TaskId`] while the finder was open; a target that is gone is
//! reported instead of used.

use std::path::PathBuf;
use std::rc::Rc;

use crate::app::entry_visibility::HiddenEntries;
use crate::app::lists::{EntryKey, TreeRow, tree_items};
use crate::app::picker::{PickerCandidate, PickerPrompt, path_name_start};
use crate::app::query::{Query, TaskData, TaskSnapshot};
use crate::app::state::{App, Focus, Mode};
use crate::app::status::StatusMessage;
use crate::app::task_visibility::CompletedTasks;
use crate::app::tasks::unfiltered_list_items;
use crate::config::ListOrder;
use crate::config::theme::Slot;
use crate::markdown::document::{TaskId, TodoDocument};
use crate::paths::portable;
use crate::storage::workspace::{Entry, Folder, List, ListPath, Workspace};
use crate::ui::outline::OutlineItem;
use crate::ui::tasks_pane::status_slot;

/// The title of the popup.
pub const GO_TO_TITLE: &str = "Go to";
/// Shown when going to a task showed the completed tasks that hid it.
const COMPLETED_SHOWN: &str = "completed tasks are shown";

/// A list as the candidates of its tasks name it.
#[derive(Debug, PartialEq, Eq)]
struct ListLocation {
    path: ListPath,
    /// The path without the extension, with `/` on every platform, such as `Home/Groceries`.
    text: String,
}

impl ListLocation {
    fn new(list: &List) -> Self {
        let folder = list.path().as_path().parent();
        let text = match folder.filter(|folder| !folder.as_os_str().is_empty()) {
            Some(folder) => format!("{}/{}", portable(folder), list.name()),
            None => list.name().to_owned(),
        };
        Self {
            path: list.path().clone(),
            text,
        }
    }
}

/// Where a candidate goes, with the text it is shown and matched as.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Target {
    /// A folder, by its path relative to the workspace root, shown as that path.
    Folder { path: PathBuf, text: String },
    /// A list, shown as its path without the extension.
    List(Rc<ListLocation>),
    /// A task of a list, shown as its description.
    Task {
        list: Rc<ListLocation>,
        id: TaskId,
        task: TaskSnapshot,
    },
}

/// A folder, list or task offered by go to anything.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GoToCandidate(Target);

impl GoToCandidate {
    fn folder(folder: &Folder) -> Self {
        Self(Target::Folder {
            path: folder.path().to_path_buf(),
            text: portable(folder.path()).to_string(),
        })
    }
}

impl PickerCandidate for GoToCandidate {
    fn text(&self) -> &str {
        match &self.0 {
            Target::Folder { text, .. } => text,
            Target::List(list) => &list.text,
            Target::Task { task, .. } => task.description(),
        }
    }

    /// Folders and lists are named by the last component of their path, tasks by all their
    /// text.
    fn name_start(&self) -> usize {
        match &self.0 {
            Target::Folder { .. } | Target::List(_) => path_name_start(self.text()),
            Target::Task { .. } => 0,
        }
    }

    /// Folders and lists are styled as in the lists pane, tasks by their status as in the
    /// tasks pane, so that completed tasks are dimmed.
    fn slot(&self) -> Slot {
        match &self.0 {
            Target::Folder { .. } => Slot::Folder,
            Target::List(_) => Slot::List,
            Target::Task { task, .. } => status_slot(task.status()),
        }
    }

    fn label(&self) -> Option<&str> {
        Some(match self.0 {
            Target::Folder { .. } => "folder",
            Target::List(_) => "list",
            Target::Task { .. } => "task",
        })
    }

    /// A task is in its list.
    fn location(&self) -> Option<&str> {
        match &self.0 {
            Target::Folder { .. } | Target::List(_) => None,
            Target::Task { list, .. } => Some(&list.text),
        }
    }

    /// Only tasks match terms.
    fn matches_terms(&self, query: &Query) -> bool {
        match &self.0 {
            Target::Folder { .. } | Target::List(_) => false,
            Target::Task { task, .. } => query.matches_terms(task),
        }
    }
}

/// The open and the completed tasks of the lists, collected apart so that the open ones are
/// offered first.
#[derive(Debug, Default)]
struct TaskCandidates {
    open: Vec<GoToCandidate>,
    completed: Vec<GoToCandidate>,
}

impl TaskCandidates {
    /// Adds every task of `document`, the content of `list`.
    fn push_tasks(&mut self, list: &Rc<ListLocation>, document: &TodoDocument) {
        for node in document.tasks() {
            let tasks = if node.task().status().is_completed() {
                &mut self.completed
            } else {
                &mut self.open
            };
            tasks.push(GoToCandidate(Target::Task {
                list: Rc::clone(list),
                id: node.id(),
                task: TaskSnapshot::from(node.task()),
            }));
        }
    }
}

/// Returns the folders and lists of `workspace` that the lists pane shows with
/// `hidden_entries`, in display order, followed by the open and then the completed tasks of
/// the lists that could be loaded.
fn go_to_candidates(workspace: &Workspace, hidden_entries: HiddenEntries) -> Vec<GoToCandidate> {
    let mut entries = Vec::new();
    let mut tasks = TaskCandidates::default();
    for item in tree_items(workspace, hidden_entries, ListOrder::Name) {
        match item.row() {
            TreeRow::Entry(Entry::Folder(folder)) => entries.push(GoToCandidate::folder(folder)),
            TreeRow::Entry(Entry::List(list)) => {
                let location = Rc::new(ListLocation::new(list));
                if let Ok(loaded) = list.content() {
                    tasks.push_tasks(&location, loaded.document());
                }
                entries.push(GoToCandidate(Target::List(location)));
            }
            TreeRow::Bin(_) => {}
        }
    }
    entries.reserve_exact(tasks.open.len() + tasks.completed.len());
    entries.extend(tasks.open);
    entries.extend(tasks.completed);
    entries
}

/// Returns the task of the list at `list` in `workspace` that a candidate with `id` and `text`
/// names: the task with `id`, or else the first task with `text`, because a reload gives every
/// task a new id. `None` if the list or the task is gone.
fn find_task(workspace: &Workspace, list: &ListPath, id: TaskId, text: &str) -> Option<TaskId> {
    let loaded = workspace.list(list)?.content().ok()?;
    let document = loaded.document();
    if document.depth(id).is_some() {
        return Some(id);
    }
    document
        .tasks()
        .find(|node| node.task().description() == text)
        .map(|node| node.id())
}

/// Where to go after `target` was looked up again in the workspace.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Destination {
    Folder(PathBuf),
    List(ListPath),
    Task(ListPath, TaskId),
}

/// Looks `target` up again in `workspace`.
///
/// # Errors
///
/// Returns a message naming the target if it is no longer in the workspace.
fn destination(workspace: &Workspace, target: &Target) -> Result<Destination, String> {
    match target {
        Target::Folder { path, text } => workspace
            .contains_folder(path)
            .then(|| Destination::Folder(path.clone()))
            .ok_or_else(|| format!("folder {text} is gone")),
        Target::List(list) => workspace
            .list(&list.path)
            .map(|_| Destination::List(list.path.clone()))
            .ok_or_else(|| format!("list {} is gone", list.text)),
        Target::Task { list, id, task } => {
            let text = task.description();
            find_task(workspace, &list.path, *id, text)
                .map(|id| Destination::Task(list.path.clone(), id))
                .ok_or_else(|| format!("task {text} is gone"))
        }
    }
}

impl App {
    /// Opens go to anything over the folders, lists and tasks the panes can show, and
    /// discards a pending key sequence.
    pub(super) fn open_go_to(&mut self) {
        self.resolver.reset();
        let candidates = go_to_candidates(&self.workspace, self.lists.hidden_entries());
        self.mode = Mode::GoTo(PickerPrompt::with_terms(candidates, self.today));
    }

    /// Closes go to anything and goes to the selected candidate; one that is gone keeps the
    /// finder open and says so, and nothing happens without a match.
    pub(super) fn submit_go_to(&mut self) {
        let Mode::GoTo(prompt) = &mut self.mode else {
            return;
        };
        let Some(GoToCandidate(target)) = prompt.picker().selected() else {
            return;
        };
        match destination(&self.workspace, target) {
            Ok(destination) => {
                self.close_go_to();
                self.go_to(destination);
            }
            Err(error) => prompt.refuse(error),
        }
    }

    /// Closes go to anything without moving and discards a pending key sequence.
    pub(super) fn close_go_to(&mut self) {
        if let Mode::GoTo(_) = self.mode {
            self.resolver.reset();
            self.mode = Mode::Normal;
        }
    }

    /// Focuses and selects `destination`: a folder in the lists pane, unfolding the folders
    /// that contain it; a list opened in the tasks pane; or a task of its opened list,
    /// unfolding its parents and revealing it if a filter or hidden completed tasks hide it.
    fn go_to(&mut self, destination: Destination) {
        match destination {
            Destination::Folder(path) => {
                let viewport_height = self.viewport_height(Focus::Lists);
                let key = EntryKey::Folder(path);
                self.lists.select(&self.workspace, &key, viewport_height);
                self.focus = Focus::Lists;
            }
            Destination::List(list) => self.open_list(&list),
            Destination::Task(list, task) => {
                self.open_list(&list);
                self.show_completed_tasks_hiding(&list, task);
                self.select_task(task);
            }
        }
    }

    /// Opens the list at `path` in the tasks pane and focuses it, as `open` does.
    fn open_list(&mut self, path: &ListPath) {
        let viewport_height = self.viewport_height(Focus::Lists);
        self.lists.open(&self.workspace, path, viewport_height);
        self.focus = Focus::Tasks;
        self.refresh_tasks();
    }

    /// Shows completed tasks if hiding them hides `task` of the list at `list`, because it
    /// or a task containing it is completed, and says so.
    fn show_completed_tasks_hiding(&mut self, list: &ListPath, task: TaskId) {
        let shown = unfiltered_list_items(&self.workspace, list, self.completed_tasks)
            .any(|item| *item.key() == task);
        if !shown {
            self.completed_tasks = CompletedTasks::Shown;
            self.show_message(StatusMessage::info(COMPLETED_SHOWN));
        }
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::Path;

    use rstest::rstest;
    use tempfile::TempDir;

    use super::*;
    use crate::app::effect::Effect;
    use crate::app::state::tests::app_in;
    use crate::config::IndentStyle;
    use crate::config::keymap::action::Action;
    use crate::storage::bin::BinPath;
    use crate::storage::workspace::{Reload, Rescan};

    /// Lists in display order: `Home/Groceries` (done `Bread`, open `Milk`), `Inbox` (`Call`),
    /// the hidden `.archive/Old` (`Old task`) and `Broken`, which is not UTF-8; the bin holds
    /// `Gone`.
    const FILES: &[(&str, &[u8])] = &[
        ("Home/Groceries.md", b"- [x] Bread\n- [ ] Milk\n"),
        ("Inbox.md", b"- [ ] Call\n"),
        (".archive/Old.md", b"- [ ] Old task\n"),
        ("Broken.md", &[0xff, 0xfe]),
        (".trash/Gone.md", b"- [ ] Thrown away\n"),
    ];

    fn write_files(root: &TempDir) {
        for (path, content) in FILES {
            let path = root.path().join(path);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, content).unwrap();
        }
    }

    fn workspace(root: &TempDir) -> Workspace {
        write_files(root);
        Workspace::scan(root.path(), BinPath::default(), IndentStyle::default()).unwrap()
    }

    fn texts(candidates: &[GoToCandidate]) -> Vec<&str> {
        candidates.iter().map(PickerCandidate::text).collect()
    }

    fn candidate(workspace: &Workspace, text: &str) -> GoToCandidate {
        go_to_candidates(workspace, HiddenEntries::Shown)
            .into_iter()
            .find(|candidate| candidate.text() == text)
            .unwrap()
    }

    fn prompt_of(app: &App) -> &PickerPrompt<GoToCandidate> {
        match app.mode() {
            Mode::GoTo(prompt) => prompt,
            mode => panic!("go to anything is not open, the mode is {mode:?}"),
        }
    }

    #[rstest]
    #[case::hidden(
        HiddenEntries::Hidden,
        &["Home", "Home/Groceries", "Broken", "Inbox", "Milk", "Call", "Bread"]
    )]
    #[case::shown(
        HiddenEntries::Shown,
        &[
            ".archive", ".archive/Old", "Home", "Home/Groceries", "Broken", "Inbox",
            "Old task", "Milk", "Call", "Bread"
        ]
    )]
    fn candidates_are_the_shown_entries_then_open_then_completed_tasks(
        #[case] hidden_entries: HiddenEntries,
        #[case] expected: &[&str],
    ) {
        let root = TempDir::new().unwrap();
        let workspace = workspace(&root);

        let system_under_test = go_to_candidates(&workspace, hidden_entries);

        assert_eq!(texts(&system_under_test), expected);
    }

    #[rstest]
    #[case::folder("Home", 0, Slot::Folder, "folder", None)]
    #[case::list("Home/Groceries", 5, Slot::List, "list", None)]
    #[case::open_task("Milk", 0, Slot::Text, "task", Some("Home/Groceries"))]
    #[case::done_task("Bread", 0, Slot::TaskDone, "task", Some("Home/Groceries"))]
    fn candidate_shows_its_kind_style_and_list(
        #[case] text: &str,
        #[case] expected_name_start: usize,
        #[case] expected_slot: Slot,
        #[case] expected_label: &str,
        #[case] expected_location: Option<&str>,
    ) {
        let root = TempDir::new().unwrap();
        let workspace = workspace(&root);
        let candidate = candidate(&workspace, text);

        let system_under_test = (
            candidate.name_start(),
            candidate.slot(),
            candidate.label(),
            candidate.location(),
        );

        assert_eq!(
            system_under_test,
            (
                expected_name_start,
                expected_slot,
                Some(expected_label),
                expected_location
            )
        );
    }

    fn target_of(workspace: &Workspace, text: &str) -> Target {
        candidate(workspace, text).0
    }

    fn list_path(workspace: &Workspace, text: &str) -> ListPath {
        match target_of(workspace, text) {
            Target::List(list) => list.path.clone(),
            target => panic!("{text} is no list but {target:?}"),
        }
    }

    fn task_id(workspace: &Workspace, text: &str) -> TaskId {
        match target_of(workspace, text) {
            Target::Task { id, .. } => id,
            target => panic!("{text} is no task but {target:?}"),
        }
    }

    #[test]
    fn destinations_are_found_again() {
        let root = TempDir::new().unwrap();
        let workspace = workspace(&root);
        let groceries = list_path(&workspace, "Home/Groceries");
        let expected = vec![
            Ok(Destination::Folder(PathBuf::from("Home"))),
            Ok(Destination::List(groceries.clone())),
            Ok(Destination::Task(groceries, task_id(&workspace, "Milk"))),
        ];

        let system_under_test: Vec<Result<Destination, String>> =
            ["Home", "Home/Groceries", "Milk"]
                .into_iter()
                .map(|text| destination(&workspace, &target_of(&workspace, text)))
                .collect();

        assert_eq!(system_under_test, expected);
    }

    #[rstest]
    #[case::folder("Home", "folder Home is gone")]
    #[case::list("Inbox", "list Inbox is gone")]
    #[case::task("Call", "task Call is gone")]
    fn removed_destination_is_reported(#[case] text: &str, #[case] expected: &str) {
        let root = TempDir::new().unwrap();
        let mut workspace = workspace(&root);
        let target = target_of(&workspace, text);
        fs::remove_dir_all(root.path().join("Home")).unwrap();
        fs::remove_file(root.path().join("Inbox.md")).unwrap();
        let _: Rescan = workspace.rescan().unwrap();

        let system_under_test = destination(&workspace, &target);

        assert_eq!(system_under_test, Err(expected.to_owned()));
    }

    #[test]
    fn task_of_a_reloaded_list_is_found_by_its_text() {
        let root = TempDir::new().unwrap();
        let mut workspace = workspace(&root);
        let list = list_path(&workspace, "Home/Groceries");
        let stale_id = task_id(&workspace, "Milk");
        fs::write(
            root.path().join("Home/Groceries.md"),
            "- [ ] Eggs\n- [ ] Milk\n",
        )
        .unwrap();
        let _: Option<Reload> = workspace.reload(&list);
        let expected = task_id(&workspace, "Milk");

        let system_under_test = find_task(&workspace, &list, stale_id, "Milk");

        assert_ne!(stale_id, expected);
        assert_eq!(system_under_test, Some(expected));
    }

    #[rstest]
    #[case::top_level("Inbox.md", "Inbox")]
    #[case::nested("Home/Groceries.md", "Home/Groceries")]
    fn list_location_is_its_path_without_the_extension(#[case] path: &str, #[case] expected: &str) {
        let root = TempDir::new().unwrap();
        let workspace = workspace(&root);
        let Some(Entry::List(list)) = workspace.entry(Path::new(path)) else {
            panic!("{path} is no list");
        };

        let system_under_test = ListLocation::new(list);

        assert_eq!(system_under_test.text, expected);
    }

    #[test]
    fn submit_without_a_match_keeps_the_finder_open() {
        let root = TempDir::new().unwrap();
        write_files(&root);
        let mut app = app_in(root.path());
        let _: Vec<Effect> = app.update(Action::GoTo);
        let _: Vec<Effect> = app.handle_paste("xyz");

        let system_under_test = app.update(Action::Submit);

        assert_eq!(system_under_test, []);
        assert_eq!(prompt_of(&app).error(), None);
    }

    #[test]
    fn closing_without_the_finder_keeps_the_mode() {
        let root = TempDir::new().unwrap();
        let mut app = app_in(root.path());
        app.open_help();
        app.close_go_to();

        let system_under_test = app.mode();

        assert!(matches!(system_under_test, Mode::Help(_)));
    }

    #[test]
    fn opening_discards_a_pending_key_sequence() {
        let root = TempDir::new().unwrap();
        write_files(&root);
        let mut app = app_in(root.path());
        let _: Vec<Effect> = app.handle_key("g".parse().unwrap());
        app.open_go_to();

        let system_under_test = app.pending_keys();

        assert_eq!(system_under_test, []);
        assert_eq!(prompt_of(&app).picker().input().value(), "");
    }
}

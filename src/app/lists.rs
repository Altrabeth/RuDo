//! The state of the lists pane: navigation in the workspace tree and the active list.
//!
//! [`ListsPane`] never copies the workspace. [`tree_items`] flattens the current
//! [`Workspace`] whenever the pane needs it, so lists that are created, renamed or reloaded
//! can never leave a stale row behind.

use std::path::{Path, PathBuf};

use crate::app::confirm::ConfirmKind;
use crate::app::entry_visibility::HiddenEntries;
use crate::app::prompt::PromptKind;
use crate::app::search::{PaneFilter, SearchItem};
use crate::app::state::{App, Focus, Mode};
use crate::app::status::StatusMessage;
use crate::app::task_delete::counted_tasks;
use crate::config::keymap::action::Action;
use crate::storage::workspace::{Entry, List, ListPath, Workspace, moved_path};
use crate::ui::outline::{FilterRole, Outcome, Outline, OutlineCommand, OutlineItem, prune};
use crate::ui::text_input::TextInput;

/// Identifies a row of the workspace tree across rescans.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum EntryKey {
    /// A folder, by its path relative to the workspace root.
    Folder(PathBuf),
    /// A list, by its path.
    List(ListPath),
}

impl EntryKey {
    /// Returns the path of the folder or list relative to the workspace root.
    #[must_use]
    pub fn path(&self) -> &Path {
        match self {
            Self::Folder(path) => path,
            Self::List(path) => path.as_path(),
        }
    }
}

impl From<&Entry> for EntryKey {
    fn from(entry: &Entry) -> Self {
        match entry {
            Entry::Folder(folder) => Self::Folder(folder.path().to_path_buf()),
            Entry::List(list) => Self::List(list.path().clone()),
        }
    }
}

/// A folder or list that was renamed or removed, which changes or ends the paths of the lists
/// at or inside it; see [`App::apply_entry_change`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(super) enum EntryChange<'path> {
    /// The entry at `from` is now at `to`, both relative to the workspace root.
    Renamed { from: &'path Path, to: &'path Path },
    /// The entry at this path, relative to the workspace root, was removed.
    Removed(&'path Path),
}

impl EntryChange<'_> {
    /// Returns the path of the list at `path` after the change: a new path if it was renamed
    /// or inside a renamed folder, `None` if it was removed, and `path` itself otherwise.
    pub(super) fn list_path(self, path: ListPath) -> Option<ListPath> {
        match self {
            Self::Renamed { from, to } => Some(path.moved(from, to).unwrap_or(path)),
            Self::Removed(removed) => (!path.as_path().starts_with(removed)).then_some(path),
        }
    }

    /// Returns the new key of the folder or list with `key`, or `None` if it was not renamed.
    fn renamed_key(self, key: &EntryKey) -> Option<EntryKey> {
        let Self::Renamed { from, to } = self else {
            return None;
        };
        match key {
            EntryKey::Folder(path) => moved_path(path, from, to).map(EntryKey::Folder),
            EntryKey::List(path) => path.moved(from, to).map(EntryKey::List),
        }
    }
}

/// A folder or list of the workspace tree with its key, nesting depth and why the filter of
/// the lists pane shows it.
#[derive(Debug)]
pub struct TreeItem<'workspace> {
    entry: &'workspace Entry,
    key: EntryKey,
    depth: usize,
    role: FilterRole,
}

impl<'workspace> TreeItem<'workspace> {
    /// Returns the folder or list of this row.
    #[must_use]
    pub fn entry(&self) -> &'workspace Entry {
        self.entry
    }

    /// Returns whether the entry matches the filter of the lists pane or only contains a
    /// match.
    #[must_use]
    pub fn role(&self) -> FilterRole {
        self.role
    }
}

impl SearchItem for TreeItem<'_> {
    /// Lists match by name; folders are only shown because they contain a match.
    fn search_text(&self) -> Option<&str> {
        match self.entry {
            Entry::Folder(_) => None,
            Entry::List(list) => Some(list.name()),
        }
    }

    fn with_role(self, role: FilterRole) -> Self {
        Self { role, ..self }
    }
}

impl OutlineItem for TreeItem<'_> {
    type Key = EntryKey;

    fn key(&self) -> &EntryKey {
        &self.key
    }

    fn depth(&self) -> usize {
        self.depth
    }
}

/// Returns the folders and lists of `workspace` that `hidden_entries` shows, in display order,
/// parents before their entries, ready for an [`Outline`].
#[must_use]
pub fn tree_items(workspace: &Workspace, hidden_entries: HiddenEntries) -> Vec<TreeItem<'_>> {
    let mut items = Vec::new();
    push_items(&mut items, workspace.entries(), 0);
    prune(items, |item| hidden_entries.shows(item.entry)).collect()
}

fn push_items<'workspace>(
    items: &mut Vec<TreeItem<'workspace>>,
    entries: &'workspace [Entry],
    depth: usize,
) {
    for entry in entries {
        items.push(TreeItem {
            entry,
            key: EntryKey::from(entry),
            depth,
            role: FilterRole::Match,
        });
        match entry {
            Entry::Folder(folder) => push_items(items, folder.entries(), depth + 1),
            Entry::List(_) => {}
        }
    }
}

/// The lists pane: the selection and folding of the workspace tree, its search filter, whether
/// it shows hidden entries and the active list, whose tasks the tasks pane shows.
#[derive(Debug, Clone)]
pub struct ListsPane {
    outline: Outline<EntryKey>,
    active: Option<ListPath>,
    filter: PaneFilter<EntryKey>,
    hidden_entries: HiddenEntries,
}

impl ListsPane {
    /// Selects and activates the first list of `workspace` in display order that
    /// `hidden_entries` shows, if there is any, in a pane `viewport_height` rows high.
    #[must_use]
    pub fn new(
        workspace: &Workspace,
        hidden_entries: HiddenEntries,
        viewport_height: usize,
    ) -> Self {
        let items = tree_items(workspace, hidden_entries);
        let mut outline = Outline::new();
        let first_list = items.iter().find_map(|item| match &item.key {
            EntryKey::List(path) => Some((&item.key, path)),
            EntryKey::Folder(_) => None,
        });
        let Some((key, path)) = first_list else {
            outline.refresh(&items, viewport_height);
            return Self {
                outline,
                active: None,
                filter: PaneFilter::default(),
                hidden_entries,
            };
        };
        match outline.select(&items, key, viewport_height) {
            // Nothing is selected or folded yet, so selecting the list always succeeds.
            Outcome::Changed | Outcome::Unchanged => {}
        }
        Self {
            active: Some(path.clone()),
            outline,
            filter: PaneFilter::default(),
            hidden_entries,
        }
    }

    /// Returns whether the pane shows hidden folders and lists.
    #[must_use]
    pub fn hidden_entries(&self) -> HiddenEntries {
        self.hidden_entries
    }

    /// Shows hidden entries of `workspace` if they are hidden and hides them otherwise, in a
    /// viewport `viewport_height` rows high; a selection that becomes hidden moves to the
    /// nearest entry that is still shown.
    pub(super) fn toggle_hidden(&mut self, workspace: &Workspace, viewport_height: usize) {
        self.hidden_entries = self.hidden_entries.toggled();
        let all = self
            .filter
            .apply(tree_items(workspace, HiddenEntries::Shown));
        let shown = self.items(workspace);
        self.outline.select_nearest(&all, &shown, viewport_height);
    }

    /// Returns the navigation state of the workspace tree.
    #[must_use]
    pub fn outline(&self) -> &Outline<EntryKey> {
        &self.outline
    }

    pub(super) fn outline_mut(&mut self) -> &mut Outline<EntryKey> {
        &mut self.outline
    }

    /// Returns the list whose tasks the tasks pane shows, or `None` if no list was opened.
    #[must_use]
    pub fn active(&self) -> Option<&ListPath> {
        self.active.as_ref()
    }

    /// Returns the search filter of the pane.
    #[must_use]
    pub fn filter(&self) -> &PaneFilter<EntryKey> {
        &self.filter
    }

    /// Returns the folders and lists of `workspace` that the pane shows with its filter, in
    /// display order.
    #[must_use]
    pub fn items<'workspace>(&self, workspace: &'workspace Workspace) -> Vec<TreeItem<'workspace>> {
        self.filter
            .apply(tree_items(workspace, self.hidden_entries))
    }

    pub(super) fn filter_mut(&mut self) -> &mut PaneFilter<EntryKey> {
        &mut self.filter
    }

    /// Filters the tree of `workspace` again after the filter changed, in a viewport
    /// `viewport_height` rows high.
    pub(super) fn refilter(&mut self, workspace: &Workspace, viewport_height: usize) {
        let hidden_entries = self.hidden_entries;
        self.filter.refilter(
            &mut self.outline,
            || tree_items(workspace, hidden_entries),
            viewport_height,
        );
    }

    /// Keeps the selection inside a viewport `viewport_height` rows high, for example after
    /// the terminal was resized.
    pub(super) fn refresh(&mut self, workspace: &Workspace, viewport_height: usize) {
        let items = self.items(workspace);
        self.outline.refresh(&items, viewport_height);
    }

    /// Returns the folder that receives a new entry, relative to the workspace root: the
    /// selected folder, the folder of the selected list, or the root (an empty path).
    pub(super) fn target_folder(&self) -> PathBuf {
        match self.outline.selected() {
            Some(EntryKey::Folder(folder)) => folder.clone(),
            Some(EntryKey::List(list)) => list.as_path().parent().map_or_default(Path::to_path_buf),
            None => PathBuf::new(),
        }
    }

    /// Returns the selected folder or list of `workspace`, or `None` if nothing is selected.
    fn selected_entry<'workspace>(
        &self,
        workspace: &'workspace Workspace,
    ) -> Option<&'workspace Entry> {
        let selected = self.outline.selected()?;
        self.items(workspace)
            .into_iter()
            .find(|item| item.key == *selected)
            .map(|item| item.entry)
    }

    /// Follows `change` of the workspace, which is already applied to `workspace`: renamed
    /// entries stay selected, folded and active, a removed active list is closed and a removed
    /// selection moves to a neighbour in a viewport `viewport_height` rows high.
    fn apply_entry_change(
        &mut self,
        workspace: &Workspace,
        change: EntryChange<'_>,
        viewport_height: usize,
    ) {
        self.outline.rename_keys(|key| change.renamed_key(key));
        self.active = self.active.take().and_then(|path| change.list_path(path));
        self.refresh(workspace, viewport_height);
    }

    /// Selects the entry with `key` in the tree of `workspace`, unfolding the folders that
    /// contain it and revealing it while a filter is active, in a viewport `viewport_height`
    /// rows high.
    pub(super) fn select(&mut self, workspace: &Workspace, key: &EntryKey, viewport_height: usize) {
        self.filter.reveal(key.clone());
        let items = self.items(workspace);
        // An entry that is already selected stays selected, so either outcome is fine.
        let _: Outcome = self.outline.select(&items, key, viewport_height);
    }

    /// Performs `action` on the tree of `workspace`, shown in a viewport `viewport_height` rows
    /// high, and returns whether it opened a list.
    ///
    /// `open` opens a selected list and folds or unfolds a selected folder; `expand` opens a
    /// list when there is nothing to unfold.
    fn update(&mut self, workspace: &Workspace, action: Action, viewport_height: usize) -> bool {
        let command = if action == Action::Open {
            if self.open_selected_list() {
                return true;
            }
            OutlineCommand::ToggleFold
        } else if let Some(command) = OutlineCommand::from_action(action) {
            command
        } else {
            return false;
        };
        let items = self.items(workspace);
        let outcome = self.outline.apply(&items, command, viewport_height);
        command == OutlineCommand::Expand
            && outcome == Outcome::Unchanged
            && self.open_selected_list()
    }

    /// Selects the list at `path` in the tree of `workspace` and makes it the active one, in a
    /// viewport `viewport_height` rows high.
    pub(super) fn open(&mut self, workspace: &Workspace, path: &ListPath, viewport_height: usize) {
        self.select(workspace, &EntryKey::List(path.clone()), viewport_height);
        self.active = Some(path.clone());
    }

    /// Makes the selected list the active one; returns `false` if no list is selected.
    fn open_selected_list(&mut self) -> bool {
        match self.outline.selected() {
            Some(EntryKey::List(path)) => {
                self.active = Some(path.clone());
                true
            }
            Some(EntryKey::Folder(_)) | None => false,
        }
    }
}

impl App {
    /// Returns the state of the lists pane, including the active list.
    #[must_use]
    pub fn lists(&self) -> &ListsPane {
        &self.lists
    }

    /// Follows a folder or list that was renamed or removed in the workspace in every piece of
    /// state that remembers lists by their [`ListPath`] or entries by their path.
    ///
    /// Every renamed or removed entry passes through here, so state keyed by [`ListPath`]
    /// remaps its keys with [`EntryChange::list_path`] in this method.
    pub(super) fn apply_entry_change(&mut self, change: EntryChange<'_>) {
        let viewport_height = self.viewport_height(Focus::Lists);
        self.lists
            .apply_entry_change(&self.workspace, change, viewport_height);
        self.tasks.apply_entry_change(change);
        self.history.apply_entry_change(change);
    }

    /// Opens a prompt to rename the selected folder or list, starting with its current name.
    pub(super) fn rename_selected_entry(&mut self) {
        let Some(entry) = self.lists.selected_entry(&self.workspace) else {
            return;
        };
        let kind = PromptKind::Rename {
            kind: entry.kind(),
            path: entry.path().to_path_buf(),
        };
        let input = TextInput::with_value(entry.name());
        self.open_prompt(kind, input);
    }

    /// Asks to confirm deleting the selected list or empty folder.
    pub(super) fn delete_selected_entry(&mut self) {
        if let Some(entry) = self.lists.selected_entry(&self.workspace) {
            self.ask_to_delete_entries(&[EntryKey::from(entry)]);
        }
    }

    /// Asks to confirm deleting the lists and empty folders `entries` that are still in the
    /// workspace, as [`Effect::DeleteEntries`](crate::app::Effect::DeleteEntries).
    ///
    /// Every deletion of lists and folders starts here. A folder with entries is never deleted
    /// and refuses the whole deletion with an error naming it.
    pub(super) fn ask_to_delete_entries(&mut self, entries: &[EntryKey]) {
        let found: Vec<&Entry> = entries
            .iter()
            .filter_map(|key| self.workspace.entry(key.path()))
            .collect();
        match delete_question(&found) {
            Ok(message) => {
                let entries = found.into_iter().map(EntryKey::from).collect();
                self.open_confirm(ConfirmKind::DeleteEntries(entries), message);
            }
            Err(error) => self.show_message(StatusMessage::error(error)),
        }
    }

    /// Opens the prompt for the folder that receives the selected lists and folders in visual
    /// mode, or the selected entry otherwise.
    pub(super) fn ask_for_move_target(&mut self) {
        let entries = match self.mode {
            Mode::Visual => match self.focus {
                Focus::Lists => self.take_selected_entries(),
                Focus::Tasks => {
                    self.show_message(StatusMessage::info(ONLY_ENTRIES_ARE_MOVED));
                    None
                }
            },
            Mode::Normal => self
                .lists
                .selected_entry(&self.workspace)
                .map(|entry| vec![EntryKey::from(entry)]),
            Mode::Help(_)
            | Mode::Prompt(_)
            | Mode::Confirm(_)
            | Mode::InlineEdit(_)
            | Mode::Search(_) => None,
        };
        if let Some(entries) = entries {
            self.open_prompt(PromptKind::MoveToFolder { entries }, TextInput::new());
        }
    }

    /// Performs a navigation, folding or `open` action in the lists pane and focuses the
    /// tasks pane if it opened a list, which shows the task it showed when it was last open.
    pub(super) fn update_lists(&mut self, action: Action) {
        let viewport_height = self.viewport_height(Focus::Lists);
        if self.lists.update(&self.workspace, action, viewport_height) {
            self.focus = Focus::Tasks;
            self.refresh_tasks();
        }
    }
}

/// Checks that `folder`, relative to the root of `workspace`, can receive the lists and folders
/// `entries`: it must be a folder of the workspace, or the root, and not one of the folders or
/// inside one of them.
///
/// # Errors
///
/// Returns why `folder` cannot receive `entries`, to be shown in the prompt.
pub(super) fn check_move_target(
    workspace: &Workspace,
    entries: &[EntryKey],
    folder: &Path,
) -> Result<(), String> {
    if !workspace.contains_folder(folder) {
        return Err(format!("there is no folder {}", folder.display()));
    }
    let moved_into_itself = entries.iter().find_map(|entry| match entry {
        EntryKey::Folder(path) if folder.starts_with(path) => Some(path),
        EntryKey::Folder(_) | EntryKey::List(_) => None,
    });
    match moved_into_itself {
        Some(path) => Err(format!("cannot move folder {} into itself", path.display())),
        None => Ok(()),
    }
}

/// Shown when `move-to-folder` is used on a selection of tasks.
const ONLY_ENTRIES_ARE_MOVED: &str = "only lists and folders can be moved into a folder";

/// Asks whether to delete `entries`, or tells why they cannot be deleted: a folder that is not
/// empty, or no entries at all.
fn delete_question(entries: &[&Entry]) -> Result<String, String> {
    let non_empty_folder = entries.iter().find_map(|entry| match entry {
        Entry::Folder(folder) if !folder.entries().is_empty() => Some(folder),
        Entry::Folder(_) | Entry::List(_) => None,
    });
    if let Some(folder) = non_empty_folder {
        return Err(format!("folder \"{}\" is not empty", folder.name()));
    }
    match entries {
        [] => Err("the selected lists are gone".to_owned()),
        [Entry::List(list)] => Ok(delete_list_question(list)),
        [Entry::Folder(folder)] => Ok(format!("Delete empty folder \"{}\"?", folder.name())),
        several => Ok(delete_several_question(several)),
    }
}

/// Asks whether to delete several lists and empty folders, counting both and the tasks of the
/// lists that could be loaded, such as `Delete 12 lists with 340 tasks and 2 empty folders?`.
fn delete_several_question(entries: &[&Entry]) -> String {
    let (lists, tasks, folders) =
        entries
            .iter()
            .fold((0, 0, 0), |(lists, tasks, folders), entry| match entry {
                Entry::List(list) => (lists + 1, tasks + task_count(list), folders),
                Entry::Folder(_) => (lists, tasks, folders + 1),
            });
    let lists = (lists > 0).then(|| {
        let noun = if lists == 1 { "list" } else { "lists" };
        format!("{lists} {noun} with {}", counted_tasks(tasks))
    });
    let folders = (folders > 0).then(|| {
        let noun = if folders == 1 { "folder" } else { "folders" };
        format!("{folders} empty {noun}")
    });
    let parts: Vec<String> = lists.into_iter().chain(folders).collect();
    format!("Delete {}?", parts.join(" and "))
}

/// Returns the number of tasks of `list`, `0` if it could not be loaded.
fn task_count(list: &List) -> usize {
    list.content()
        .map_or(0, |loaded| loaded.document().progress().total())
}

/// Asks whether to delete `list`, with its number of tasks if it could be loaded.
fn delete_list_question(list: &List) -> String {
    let name = list.name();
    match list.content() {
        Ok(loaded) => {
            let tasks = loaded.document().progress().total();
            let noun = if tasks == 1 { "task" } else { "tasks" };
            format!("Delete list \"{name}\" with {tasks} {noun}?")
        }
        Err(_) => format!("Delete list \"{name}\"?"),
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use rstest::rstest;
    use tempfile::TempDir;

    use super::*;
    use crate::config::IndentStyle;

    /// A workspace with the empty folders `empty` and `other`, the folder `home` with
    /// `chores` (two tasks) and the lists `inbox` (one task) and `binary`, which cannot be
    /// loaded.
    fn workspace(root: &TempDir) -> Workspace {
        for folder in ["empty", "other", "home"] {
            fs::create_dir(root.path().join(folder)).unwrap();
        }
        fs::write(root.path().join("home/chores.md"), "- [ ] a\n- [ ] b\n").unwrap();
        fs::write(root.path().join("inbox.md"), "- [ ] c\n").unwrap();
        fs::write(root.path().join("binary.md"), [0xff]).unwrap();
        Workspace::scan(root.path(), IndentStyle::default()).unwrap()
    }

    fn entries<'workspace>(
        workspace: &'workspace Workspace,
        paths: &str,
    ) -> Vec<&'workspace Entry> {
        paths
            .split_whitespace()
            .map(|path| workspace.entry(Path::new(path)).unwrap())
            .collect()
    }

    #[rstest]
    #[case::one_list("inbox.md", Ok("Delete list \"inbox\" with 1 task?"))]
    #[case::one_empty_folder("empty", Ok("Delete empty folder \"empty\"?"))]
    #[case::lists("inbox.md home/chores.md", Ok("Delete 2 lists with 3 tasks?"))]
    #[case::unreadable_list_counts_no_tasks(
        "inbox.md binary.md",
        Ok("Delete 2 lists with 1 task?")
    )]
    #[case::folders("empty other", Ok("Delete 2 empty folders?"))]
    #[case::both("empty inbox.md", Ok("Delete 1 list with 1 task and 1 empty folder?"))]
    #[case::non_empty_folder("inbox.md home", Err("folder \"home\" is not empty"))]
    #[case::nothing("", Err("the selected lists are gone"))]
    fn delete_question_counts_lists_tasks_and_folders(
        #[case] paths: &str,
        #[case] expected: Result<&str, &str>,
    ) {
        let root = TempDir::new().unwrap();
        let workspace = workspace(&root);
        let entries = entries(&workspace, paths);
        let expected = expected.map(str::to_owned).map_err(str::to_owned);

        let system_under_test = delete_question(&entries);

        assert_eq!(system_under_test, expected);
    }

    #[rstest]
    #[case::folder("inbox.md", "home", Ok(()))]
    #[case::root("home/chores.md", "", Ok(()))]
    #[case::folder_into_another("home", "empty", Ok(()))]
    #[case::list_is_no_folder("home", "inbox.md", Err("there is no folder inbox.md"))]
    #[case::missing("inbox.md", "garden", Err("there is no folder garden"))]
    #[case::itself("home", "home", Err("cannot move folder home into itself"))]
    fn move_target_must_be_another_folder(
        #[case] path: &str,
        #[case] folder: &str,
        #[case] expected: Result<(), &str>,
    ) {
        let root = TempDir::new().unwrap();
        let workspace = workspace(&root);
        let entries: Vec<EntryKey> = entries(&workspace, path)
            .into_iter()
            .map(EntryKey::from)
            .collect();
        let expected = expected.map_err(str::to_owned);

        let system_under_test = check_move_target(&workspace, &entries, Path::new(folder));

        assert_eq!(system_under_test, expected);
    }

    #[test]
    fn tree_items_flatten_folders_before_their_entries_with_depths() {
        let root = TempDir::new().unwrap();
        fs::create_dir_all(root.path().join("home/empty")).unwrap();
        fs::write(root.path().join("home/chores.md"), "").unwrap();
        fs::write(root.path().join("inbox.md"), "").unwrap();
        let workspace = Workspace::scan(root.path(), IndentStyle::default()).unwrap();
        let expected = [("home", 0), ("empty", 1), ("chores", 1), ("inbox", 0)];

        let system_under_test: Vec<(&str, usize)> = tree_items(&workspace, HiddenEntries::Hidden)
            .iter()
            .map(|item| (item.entry().name(), item.depth()))
            .collect();

        assert_eq!(system_under_test, expected);
    }

    #[rstest]
    #[case::shown(
        HiddenEntries::Shown,
        &[(".obsidian", 0), ("daily", 1), ("home", 0), (".drafts", 1), ("chores", 1)]
    )]
    #[case::hidden(HiddenEntries::Hidden, &[("home", 0), ("chores", 1)])]
    fn tree_items_leave_out_hidden_entries_while_they_are_hidden(
        #[case] hidden_entries: HiddenEntries,
        #[case] expected: &[(&str, usize)],
    ) {
        let root = TempDir::new().unwrap();
        fs::create_dir_all(root.path().join(".obsidian")).unwrap();
        fs::create_dir_all(root.path().join("home")).unwrap();
        fs::write(root.path().join(".obsidian/daily.md"), "").unwrap();
        fs::write(root.path().join("home/.drafts.md"), "").unwrap();
        fs::write(root.path().join("home/chores.md"), "").unwrap();
        let workspace = Workspace::scan(root.path(), IndentStyle::default()).unwrap();

        let system_under_test: Vec<(&str, usize)> = tree_items(&workspace, hidden_entries)
            .iter()
            .map(|item| (item.entry().name(), item.depth()))
            .collect();

        assert_eq!(system_under_test, expected);
    }
}

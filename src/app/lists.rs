//! The state of the lists pane: navigation in the workspace tree and the active list.
//!
//! [`ListsPane`] never copies the workspace. [`tree_items`] flattens the current
//! [`Workspace`] whenever the pane needs it, so lists that are created, renamed or reloaded
//! can never leave a stale row behind.

use std::path::{Path, PathBuf};

use crate::app::confirm::ConfirmKind;
use crate::app::prompt::PromptKind;
use crate::app::state::{App, Focus};
use crate::app::status::StatusMessage;
use crate::config::keymap::action::Action;
use crate::storage::workspace::{Entry, List, ListPath, Workspace, moved_path};
use crate::ui::outline::{Outcome, Outline, OutlineCommand, OutlineItem};
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

/// A folder or list of the workspace tree with its key and nesting depth.
#[derive(Debug)]
pub struct TreeItem<'workspace> {
    entry: &'workspace Entry,
    key: EntryKey,
    depth: usize,
}

impl<'workspace> TreeItem<'workspace> {
    /// Returns the folder or list of this row.
    #[must_use]
    pub fn entry(&self) -> &'workspace Entry {
        self.entry
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

/// Returns every folder and list of `workspace` in display order, parents before their
/// entries, ready for an [`Outline`].
#[must_use]
pub fn tree_items(workspace: &Workspace) -> Vec<TreeItem<'_>> {
    let mut items = Vec::new();
    push_items(&mut items, workspace.entries(), 0);
    items
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
        });
        match entry {
            Entry::Folder(folder) => push_items(items, folder.entries(), depth + 1),
            Entry::List(_) => {}
        }
    }
}

/// The lists pane: the selection and folding of the workspace tree and the active list, whose
/// tasks the tasks pane shows.
#[derive(Debug, Clone)]
pub struct ListsPane {
    outline: Outline<EntryKey>,
    active: Option<ListPath>,
}

impl ListsPane {
    /// Selects and activates the first list of `workspace` in display order, if it has any,
    /// in a pane `viewport_height` rows high.
    #[must_use]
    pub fn new(workspace: &Workspace, viewport_height: usize) -> Self {
        let items = tree_items(workspace);
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
            };
        };
        match outline.select(&items, key, viewport_height) {
            // Nothing is selected or folded yet, so selecting the list always succeeds.
            Outcome::Changed | Outcome::Unchanged => {}
        }
        Self {
            active: Some(path.clone()),
            outline,
        }
    }

    /// Returns the navigation state of the workspace tree.
    #[must_use]
    pub fn outline(&self) -> &Outline<EntryKey> {
        &self.outline
    }

    /// Returns the list whose tasks the tasks pane shows, or `None` if no list was opened.
    #[must_use]
    pub fn active(&self) -> Option<&ListPath> {
        self.active.as_ref()
    }

    /// Keeps the selection inside a viewport `viewport_height` rows high, for example after
    /// the terminal was resized.
    pub(super) fn refresh(&mut self, workspace: &Workspace, viewport_height: usize) {
        self.outline
            .refresh(&tree_items(workspace), viewport_height);
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
        tree_items(workspace)
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
    /// contain it, in a viewport `viewport_height` rows high.
    pub(super) fn select(&mut self, workspace: &Workspace, key: &EntryKey, viewport_height: usize) {
        // An entry that is already selected stays selected, so either outcome is fine.
        let _: Outcome = self
            .outline
            .select(&tree_items(workspace), key, viewport_height);
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
        let outcome = self
            .outline
            .apply(&tree_items(workspace), command, viewport_height);
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

    /// Asks to confirm deleting the selected list or empty folder; a folder with entries is
    /// never deleted and shows an error instead.
    pub(super) fn delete_selected_entry(&mut self) {
        let Some(entry) = self.lists.selected_entry(&self.workspace) else {
            return;
        };
        let message = match entry {
            Entry::List(list) => delete_list_question(list),
            Entry::Folder(folder) if folder.entries().is_empty() => {
                format!("Delete empty folder \"{}\"?", folder.name())
            }
            Entry::Folder(folder) => {
                let error = format!("folder \"{}\" is not empty", folder.name());
                self.show_message(StatusMessage::error(error));
                return;
            }
        };
        let kind = ConfirmKind::DeleteEntry {
            kind: entry.kind(),
            path: entry.path().to_path_buf(),
        };
        self.open_confirm(kind, message);
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

    use tempfile::TempDir;

    use super::*;
    use crate::config::IndentStyle;

    #[test]
    fn tree_items_flatten_folders_before_their_entries_with_depths() {
        let root = TempDir::new().unwrap();
        fs::create_dir_all(root.path().join("home/empty")).unwrap();
        fs::write(root.path().join("home/chores.md"), "").unwrap();
        fs::write(root.path().join("inbox.md"), "").unwrap();
        let workspace = Workspace::scan(root.path(), IndentStyle::default()).unwrap();
        let expected = [("home", 0), ("empty", 1), ("chores", 1), ("inbox", 0)];

        let system_under_test: Vec<(&str, usize)> = tree_items(&workspace)
            .iter()
            .map(|item| (item.entry().name(), item.depth()))
            .collect();

        assert_eq!(system_under_test, expected);
    }
}

//! The state of the lists pane: navigation in the workspace tree and the active list.
//!
//! [`ListsPane`] never copies the workspace. [`tree_items`] flattens the current
//! [`Workspace`] whenever the pane needs it, so lists that are created, renamed or reloaded
//! can never leave a stale row behind.

use std::path::PathBuf;

use crate::app::state::{App, Focus};
use crate::config::keymap::action::Action;
use crate::storage::workspace::{Entry, ListPath, Workspace};
use crate::ui::outline::{Outcome, Outline, OutlineCommand, OutlineItem};

/// Identifies a row of the workspace tree across rescans.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum EntryKey {
    /// A folder, by its path relative to the workspace root.
    Folder(PathBuf),
    /// A list, by its path.
    List(ListPath),
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
        match entry {
            Entry::Folder(folder) => {
                items.push(TreeItem {
                    entry,
                    key: EntryKey::Folder(folder.path().to_path_buf()),
                    depth,
                });
                push_items(items, folder.entries(), depth + 1);
            }
            Entry::List(list) => items.push(TreeItem {
                entry,
                key: EntryKey::List(list.path().clone()),
                depth,
            }),
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

    /// Performs a navigation, folding or `open` action in the lists pane and focuses the
    /// tasks pane if it opened a list.
    pub(super) fn update_lists(&mut self, action: Action) {
        let viewport_height = self.viewport_height(Focus::Lists);
        if self.lists.update(&self.workspace, action, viewport_height) {
            self.focus = Focus::Tasks;
        }
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

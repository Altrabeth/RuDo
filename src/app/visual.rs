//! Visual mode ([`Mode::Visual`]): selecting several rows of the focused pane or of the bin
//! view, like vim's linewise `V`, and deleting, yanking, moving or restoring them in one step.
//!
//! The selection lives in the [`Outline`] of the pane or of the bin view as an anchor key, so
//! it follows edits and reloads like the cursor does. It is resolved against the rows the pane
//! shows whenever it is needed: folded rows are selected with their parent, rows hidden by a
//! filter or by hidden completed tasks are not selected, and rows shown only as the context of
//! a search match are left out as well. Every action on the selection, and every dialog it
//! opens, ends visual mode.

use std::collections::HashSet;

use crate::app::confirm::ConfirmKind;
use crate::app::effect::Effect;
use crate::app::lists::{EntryKey, TreeItem};
use crate::app::state::{App, Focus, Mode};
use crate::app::status::StatusMessage;
use crate::app::task_delete::counted_tasks;
use crate::app::tasks::{TaskItem, list_items};
use crate::markdown::document::{TaskBlock, TaskId, TodoDocument};
use crate::storage::workspace::{List, ListPath};
use crate::ui::outline::{FilterRole, Outline, OutlineItem, OutlineRow};

/// Shown when an action finds the selection gone, for example after a reload removed its
/// anchor.
const SELECTION_GONE: &str = "the selection is gone";
/// Shown when the selection holds only rows that are shown as the context of a search match.
const NOTHING_SELECTED: &str = "nothing selected";
/// Shown when `yank-selection` is used on lists and folders.
const ONLY_TASKS_ARE_YANKED: &str = "only tasks can be yanked";

impl App {
    /// Returns the number of rows selected in visual mode, or `None` outside visual mode or if
    /// the anchor of the selection is gone.
    #[must_use]
    pub fn visual_selection_count(&self) -> Option<usize> {
        match self.mode {
            Mode::Visual => match self.focus {
                Focus::Lists if self.lists.bin_view().is_some() => self
                    .selected_bin_locations()
                    .map(|locations| locations.len()),
                Focus::Lists => self.selected_entries().map(|entries| entries.len()),
                Focus::Tasks => self.selected_tasks().map(|(_, tasks)| tasks.len()),
            },
            Mode::Normal
            | Mode::Help(_)
            | Mode::Prompt(_)
            | Mode::Confirm(_)
            | Mode::InlineEdit(_)
            | Mode::Search(_) => None,
        }
    }

    /// Starts visual mode at the cursor of the focused pane, or ends it.
    pub(super) fn toggle_visual_mode(&mut self) {
        match self.mode {
            Mode::Normal => self.start_visual_mode(),
            Mode::Visual => self.end_visual_mode(),
            Mode::Help(_)
            | Mode::Prompt(_)
            | Mode::Confirm(_)
            | Mode::InlineEdit(_)
            | Mode::Search(_) => {}
        }
    }

    /// Anchors a selection at the cursor of the focused pane, or of the bin view while it is
    /// open, and enters visual mode; a pane without rows stays in normal mode.
    fn start_visual_mode(&mut self) {
        let anchored = match self.focus {
            Focus::Lists if self.lists.bin_view().is_some() => self.start_bin_selection(),
            Focus::Lists => {
                let outline = self.lists.outline_mut();
                outline.start_selection();
                outline.anchor().is_some()
            }
            Focus::Tasks => self.lists.active().is_some_and(|list| {
                let outline = self.tasks.outline_mut(list);
                outline.start_selection();
                outline.anchor().is_some()
            }),
        };
        if anchored {
            self.mode = Mode::Visual;
        }
    }

    /// Ends the selection and returns to normal mode.
    pub(super) fn end_visual_mode(&mut self) {
        self.lists.outline_mut().end_selection();
        self.end_bin_selection();
        if let Some(list) = self.lists.active() {
            self.tasks.outline_mut(list).end_selection();
        }
        match self.mode {
            Mode::Visual => self.mode = Mode::Normal,
            Mode::Normal
            | Mode::Help(_)
            | Mode::Prompt(_)
            | Mode::Confirm(_)
            | Mode::InlineEdit(_)
            | Mode::Search(_) => {}
        }
    }

    /// Ends visual mode with a message if a change of the workspace, such as a reload, removed
    /// or hid the anchor of the selection.
    pub(super) fn end_visual_mode_without_anchor(&mut self) {
        if matches!(self.mode, Mode::Visual) && self.visual_selection_count().is_none() {
            self.end_visual_mode();
            self.show_message(StatusMessage::info(SELECTION_GONE));
        }
    }

    /// Deletes the selected tasks, or asks to delete the selected lists and empty folders, or
    /// the selected entries of the bin permanently.
    pub(super) fn delete_selection(&mut self) -> Vec<Effect> {
        match self.focus {
            Focus::Lists if self.lists.bin_view().is_some() => {
                self.ask_to_delete_from_bin();
                Vec::new()
            }
            Focus::Lists => {
                if let Some(entries) = self.take_selected_entries() {
                    self.ask_to_delete_entries(&entries);
                }
                Vec::new()
            }
            Focus::Tasks => self.delete_selected_tasks(),
        }
    }

    /// Copies the selected tasks with their subtasks into the register.
    pub(super) fn yank_selection(&mut self) {
        match self.focus {
            Focus::Lists => self.show_message(StatusMessage::info(ONLY_TASKS_ARE_YANKED)),
            Focus::Tasks => {
                let Some((list, tasks)) = self.take_selected_tasks() else {
                    return;
                };
                let ids: HashSet<TaskId> = tasks.into_iter().collect();
                let blocks = self
                    .document(&list)
                    .map_or_else(Vec::new, |document| document.copy_blocks(&ids));
                self.yank_blocks(blocks);
            }
        }
    }

    /// Ends visual mode and returns the selected lists and folders, or shows why there are
    /// none and returns `None`.
    pub(super) fn take_selected_entries(&mut self) -> Option<Vec<EntryKey>> {
        let selection = self.selected_entries();
        self.end_visual_mode();
        self.nonempty_selection(selection)
    }

    /// Deletes the selected tasks with their subtasks, after asking if there are several.
    fn delete_selected_tasks(&mut self) -> Vec<Effect> {
        let Some((list, tasks)) = self.take_selected_tasks() else {
            return Vec::new();
        };
        if let [_] = tasks[..] {
            return self.delete_tasks(&list, &tasks);
        }
        let ids: HashSet<TaskId> = tasks.iter().copied().collect();
        let task_count = self.document(&list).map_or(0, |document| {
            document
                .copy_blocks(&ids)
                .iter()
                .map(TaskBlock::task_count)
                .sum()
        });
        let message = format!("Delete {} (including subtasks)?", counted_tasks(task_count));
        self.open_confirm(ConfirmKind::DeleteTasks { list, tasks }, message);
        Vec::new()
    }

    /// Ends visual mode, moves the cursor to the first selected task, so that it lands where
    /// the selection began once the tasks are gone, and returns the active list with the
    /// selected tasks; `None` with a message if there are none.
    fn take_selected_tasks(&mut self) -> Option<(ListPath, Vec<TaskId>)> {
        let (list, tasks) = self.selected_tasks().unzip();
        self.end_visual_mode();
        let tasks = self.nonempty_selection(tasks)?;
        self.select_task(tasks[0]);
        Some((list?, tasks))
    }

    /// Returns `selection` if it holds keys, otherwise shows why it is empty or gone.
    pub(super) fn nonempty_selection<K>(&mut self, selection: Option<Vec<K>>) -> Option<Vec<K>> {
        match selection {
            Some(keys) if !keys.is_empty() => Some(keys),
            Some(_) => {
                self.show_message(StatusMessage::info(NOTHING_SELECTED));
                None
            }
            None => {
                self.show_message(StatusMessage::info(SELECTION_GONE));
                None
            }
        }
    }

    /// Returns the lists and folders selected in the lists pane, never the row of the bin, or
    /// `None` without a selection there.
    fn selected_entries(&self) -> Option<Vec<EntryKey>> {
        let items = self.lists.items(&self.workspace);
        let mut keys = selected_keys(self.lists.outline(), &items, TreeItem::role)?;
        keys.retain(|key| match key {
            EntryKey::Folder(_) | EntryKey::List(_) => true,
            EntryKey::Bin(_) => false,
        });
        Some(keys)
    }

    /// Returns the active list with the tasks selected in its tasks pane, or `None` without a
    /// selection there.
    fn selected_tasks(&self) -> Option<(ListPath, Vec<TaskId>)> {
        let list = self.lists.active()?;
        let items = list_items(
            &self.workspace,
            list,
            self.completed_tasks,
            self.tasks.filter(list),
        );
        let tasks = selected_keys(self.tasks.outline(list)?, &items, TaskItem::role)?;
        Some((list.clone(), tasks))
    }

    /// Returns the document of the list at `list`, or `None` if it is missing or could not be
    /// loaded.
    fn document(&self, list: &ListPath) -> Option<&TodoDocument> {
        let loaded = self.workspace.list(list).map(List::content)?.ok()?;
        Some(loaded.document())
    }
}

/// Returns the keys of the rows of `items` in the visual selection of `outline`, leaving out
/// those that `role` says are only the context of a search match; `None` without a
/// selection.
pub(super) fn selected_keys<I: OutlineItem>(
    outline: &Outline<I::Key>,
    items: &[I],
    role: impl Fn(&I) -> FilterRole,
) -> Option<Vec<I::Key>> {
    let rows = outline.visible_rows(items);
    let range = outline.selection_rows(&rows)?;
    Some(
        rows[range]
            .iter()
            .map(OutlineRow::item)
            .filter(|item| role(item) == FilterRole::Match)
            .map(|item| item.key().clone())
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use rstest::rstest;
    use tempfile::TempDir;

    use super::*;
    use crate::app::state::tests::app_in;

    #[rstest]
    #[case::lists(Focus::Lists)]
    #[case::tasks(Focus::Tasks)]
    fn empty_pane_stays_in_normal_mode(#[case] focus: Focus) {
        let directory = TempDir::new().unwrap();
        let mut app = app_in(directory.path());
        app.focus = focus;
        app.toggle_visual_mode();

        let system_under_test = app.mode();

        assert!(matches!(system_under_test, Mode::Normal));
        assert_eq!(app.visual_selection_count(), None);
    }

    #[test]
    fn ending_visual_mode_outside_it_keeps_the_mode() {
        let directory = TempDir::new().unwrap();
        let mut app = app_in(directory.path());
        app.open_help();
        app.end_visual_mode();

        let system_under_test = app.mode();

        assert!(matches!(system_under_test, Mode::Help(_)));
    }
}

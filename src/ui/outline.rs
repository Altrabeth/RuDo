//! Tree navigation shared by every tree-shaped pane: selection, folding and scrolling.
//!
//! The lists pane, the tasks pane and the help screen all show a flat sequence of rows that
//! encodes a tree through the depth of each row. [`Outline`] holds the navigation state of such
//! a pane and changes it through [`OutlineCommand`]s. It never owns the items: every operation
//! takes the current items, so edits, filtering and reloads cannot leave a stale copy behind.
//!
//! The selection is remembered by key instead of by row index, so it stays on the same entry
//! while rows above it are inserted, removed or folded away. A visual selection, vim's
//! linewise `V`, is remembered the same way: by the key of its anchor, from which it spans the
//! visible rows to the selected item.

use std::collections::HashSet;
use std::hash::Hash;
use std::ops::Range;

use strum::EnumIter;

use crate::config::keymap::action::Action;

/// Rows kept visible between the selection and the edge of the viewport (vim's `scrolloff`).
const SCROLL_MARGIN: usize = 2;

/// An entry of a tree that is listed in document order, parents before their descendants.
///
/// An item is the child of the nearest preceding item with a smaller depth; this is the same
/// rule Markdown uses for nested lists.
pub trait OutlineItem {
    /// Identifies the item across edits and reloads.
    type Key: Clone + Eq + Hash;

    /// Returns the key that identifies this item.
    fn key(&self) -> &Self::Key;

    /// Returns the nesting level of this item; top-level items have depth `0`.
    fn depth(&self) -> usize;
}

/// The fold state of a visible row, which decides its fold marker.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, EnumIter)]
pub enum Fold {
    /// The item has no children and cannot be folded.
    Leaf,
    /// The item has children and shows them.
    Expanded,
    /// The item has children and hides them.
    Collapsed,
}

/// Whether the folds of an [`Outline`] hide rows; see [`Outline::set_folding`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, EnumIter)]
pub enum Folding {
    /// Folded items hide their descendants.
    Applied,
    /// The folds are put aside, for example while a search filter shows every match; folds
    /// made meanwhile are dropped when the folds are applied again.
    Suspended,
}

/// Why an item is shown while a search filter is active; see [`keep_matches`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, EnumIter)]
pub enum FilterRole {
    /// The item matches the filter, or no filter is active.
    #[default]
    Match,
    /// The item does not match but is shown as the ancestor of a match.
    Context,
}

/// A navigation or folding operation on an [`Outline`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, EnumIter)]
pub enum OutlineCommand {
    /// Selects the next visible row.
    Down,
    /// Selects the previous visible row.
    Up,
    /// Selects the first row.
    Top,
    /// Selects the last visible row.
    Bottom,
    /// Moves the selection and the viewport half a viewport down.
    HalfPageDown,
    /// Moves the selection and the viewport half a viewport up.
    HalfPageUp,
    /// Selects the parent of the selected item.
    Parent,
    /// Folds the selected item, or selects its parent if it is not an unfolded parent.
    Collapse,
    /// Unfolds the selected item.
    Expand,
    /// Folds or unfolds the selected item.
    ToggleFold,
    /// Folds every item that has children.
    FoldAll,
    /// Unfolds every item.
    UnfoldAll,
}

impl OutlineCommand {
    /// Returns the command that `action` performs in a tree-shaped pane, or `None` if it is
    /// not a navigation or folding action.
    ///
    /// Every pane built on [`Outline`] maps its actions through this function, so the keys
    /// move and fold the same way everywhere. [`OutlineCommand::Parent`] has no action of its
    /// own; [`Action::Collapse`] falls back to it.
    #[must_use]
    pub const fn from_action(action: Action) -> Option<Self> {
        match action {
            Action::MoveDown => Some(Self::Down),
            Action::MoveUp => Some(Self::Up),
            Action::MoveToTop => Some(Self::Top),
            Action::MoveToBottom => Some(Self::Bottom),
            Action::HalfPageDown => Some(Self::HalfPageDown),
            Action::HalfPageUp => Some(Self::HalfPageUp),
            Action::Collapse => Some(Self::Collapse),
            Action::Expand => Some(Self::Expand),
            Action::ToggleFold => Some(Self::ToggleFold),
            Action::FoldAll => Some(Self::FoldAll),
            Action::UnfoldAll => Some(Self::UnfoldAll),
            Action::Help
            | Action::Quit
            | Action::RestoreFromBin
            | Action::EmptyBin
            | Action::CloseBin
            | Action::FocusNext
            | Action::FocusPrevious
            | Action::FocusLists
            | Action::PickerNext
            | Action::PickerPrevious
            | Action::Complete
            | Action::FocusTasks
            | Action::Search
            | Action::GoTo
            | Action::ClearFilter
            | Action::Undo
            | Action::Redo
            | Action::ToggleCompleted
            | Action::OpenInEditor
            | Action::Open
            | Action::NewList
            | Action::NewFolder
            | Action::Rename
            | Action::MoveToFolder
            | Action::ToggleHidden
            | Action::Delete
            | Action::NewTaskBelow
            | Action::NewTaskAbove
            | Action::NewSubtask
            | Action::Edit
            | Action::ToggleDone
            | Action::Yank
            | Action::PasteBelow
            | Action::PasteAbove
            | Action::MoveTaskDown
            | Action::MoveTaskUp
            | Action::Indent
            | Action::Outdent
            | Action::PriorityUp
            | Action::PriorityDown
            | Action::SetDueDate
            | Action::SetRecurrence
            | Action::SortByPriority
            | Action::SortByDueDate
            | Action::SortAlphabetically
            | Action::SortByStatus
            | Action::SortByOpenTasks
            | Action::SortByTotalTasks
            | Action::VisualMode
            | Action::DeleteSelection
            | Action::YankSelection
            | Action::CloseHelp
            | Action::ConfirmYes
            | Action::ConfirmNo
            | Action::Submit
            | Action::Cancel => None,
        }
    }
}

/// Tells the caller whether an operation changed the selection or the folding.
///
/// Panes give unchanged operations a meaning of their own: collapsing a top-level task moves
/// the focus to the lists pane, and expanding a list opens it.
#[must_use]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Outcome {
    /// The selection or the folding changed.
    Changed,
    /// Nothing changed.
    Unchanged,
}

/// A visible row of an [`Outline`]: an item that is not hidden by a folded ancestor.
#[derive(Debug)]
pub struct OutlineRow<'a, I> {
    index: usize,
    item: &'a I,
    fold: Fold,
}

impl<'a, I> OutlineRow<'a, I> {
    /// Returns the item shown in this row.
    #[must_use]
    pub fn item(&self) -> &'a I {
        self.item
    }

    /// Returns whether the item has children and whether they are hidden.
    #[must_use]
    pub fn fold(&self) -> Fold {
        self.fold
    }
}

/// The navigation state of a tree-shaped pane: selected key, scroll offset and folded keys.
///
/// Operations take the current items in document order and the height of the viewport in
/// rows. After every operation the selection is a visible row inside the viewport, or there
/// is no selection because there are no items.
///
/// # Examples
///
/// ```
/// use rudo::ui::outline::{Outline, OutlineCommand, OutlineItem, Outcome};
///
/// struct Entry {
///     name: &'static str,
///     depth: usize,
/// }
///
/// impl OutlineItem for Entry {
///     type Key = &'static str;
///
///     fn key(&self) -> &Self::Key {
///         &self.name
///     }
///
///     fn depth(&self) -> usize {
///         self.depth
///     }
/// }
///
/// let items = [
///     Entry { name: "groceries", depth: 0 },
///     Entry { name: "milk", depth: 1 },
///     Entry { name: "work", depth: 0 },
/// ];
/// let mut outline = Outline::new();
/// outline.refresh(&items, 10);
/// assert_eq!(outline.apply(&items, OutlineCommand::Collapse, 10), Outcome::Changed);
/// assert_eq!(outline.apply(&items, OutlineCommand::Down, 10), Outcome::Changed);
/// assert_eq!(outline.selected(), Some(&"work"));
/// ```
#[derive(Debug, Clone)]
pub struct Outline<K> {
    selected: Option<K>,
    /// Row of the selection at the last update; the fallback when the selected item vanishes.
    selected_row: usize,
    offset: usize,
    collapsed: HashSet<K>,
    /// The folds put aside while [`Folding::Suspended`].
    suspended: Option<HashSet<K>>,
    /// Where the visual selection started; see [`Outline::start_selection`].
    anchor: Option<K>,
}

impl<K> Default for Outline<K> {
    fn default() -> Self {
        Self {
            selected: None,
            selected_row: 0,
            offset: 0,
            collapsed: HashSet::new(),
            suspended: None,
            anchor: None,
        }
    }
}

impl<K: Clone + Eq + Hash> Outline<K> {
    /// Creates an outline without selection, scrolled to the top and with nothing folded.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns the key of the selected item, or `None` if the outline has no items.
    #[must_use]
    pub fn selected(&self) -> Option<&K> {
        self.selected.as_ref()
    }

    /// Returns the index of the first visible row shown at the top of the viewport.
    #[must_use]
    pub fn offset(&self) -> usize {
        self.offset
    }

    /// Returns the key of the item where the visual selection started, or `None` without a
    /// visual selection.
    #[must_use]
    pub fn anchor(&self) -> Option<&K> {
        self.anchor.as_ref()
    }

    /// Starts a visual selection at the selected item: from now on the selection spans the
    /// visible rows from this anchor to the selected item, wherever navigation moves it.
    ///
    /// Without a selected item there is nothing to anchor, and no selection starts.
    pub fn start_selection(&mut self) {
        self.anchor.clone_from(&self.selected);
    }

    /// Ends the visual selection.
    pub fn end_selection(&mut self) {
        self.anchor = None;
    }

    /// Returns the indices among `rows`, the [`Outline::visible_rows`] of the current items,
    /// of the visual selection: the contiguous rows from the anchor to the selected item, both
    /// included, in either order.
    ///
    /// Returns `None` without a visual selection, or if the anchor or the selected item is
    /// not among `rows`, for example because a reload removed it.
    #[must_use]
    pub fn selection_rows<I: OutlineItem<Key = K>>(
        &self,
        rows: &[OutlineRow<'_, I>],
    ) -> Option<Range<usize>> {
        let row_of = |key: &K| rows.iter().position(|row| row.item.key() == key);
        let anchor = row_of(self.anchor.as_ref()?)?;
        let cursor = row_of(self.selected.as_ref()?)?;
        Some(anchor.min(cursor)..anchor.max(cursor) + 1)
    }

    /// Returns the rows of `items` that are not hidden by a folded ancestor, in order.
    #[must_use]
    pub fn visible_rows<'a, I: OutlineItem<Key = K>>(
        &self,
        items: &'a [I],
    ) -> Vec<OutlineRow<'a, I>> {
        visible_rows(items, &self.collapsed)
    }

    /// Brings the state in line with changed items or a resized viewport.
    ///
    /// A selected item that was removed hands the selection to the item now in its row, or
    /// to the last row; a selected item hidden by a folded ancestor hands it to that ancestor.
    /// Without a selection the first row is selected.
    pub fn refresh<I: OutlineItem<Key = K>>(&mut self, items: &[I], viewport_height: usize) {
        self.refreshed_rows(items, viewport_height);
    }

    /// Executes `command` on `items` shown in a viewport `viewport_height` rows high.
    pub fn apply<I: OutlineItem<Key = K>>(
        &mut self,
        items: &[I],
        command: OutlineCommand,
        viewport_height: usize,
    ) -> Outcome {
        let rows = self.refreshed_rows(items, viewport_height);
        if rows.is_empty() {
            return Outcome::Unchanged;
        }
        let current = self.selected_row;
        let outcome = match command {
            OutlineCommand::Down => self.move_to(&rows, current.saturating_add(1)),
            OutlineCommand::Up => self.move_to(&rows, current.saturating_sub(1)),
            OutlineCommand::Top => self.move_to(&rows, 0),
            OutlineCommand::Bottom => self.move_to(&rows, usize::MAX),
            OutlineCommand::HalfPageDown => {
                let step = half_page(viewport_height);
                self.offset = self.offset.saturating_add(step);
                self.move_to(&rows, current.saturating_add(step))
            }
            OutlineCommand::HalfPageUp => {
                let step = half_page(viewport_height);
                self.offset = self.offset.saturating_sub(step);
                self.move_to(&rows, current.saturating_sub(step))
            }
            OutlineCommand::Parent => self.move_to_parent(&rows),
            OutlineCommand::Collapse => self.collapse(&rows),
            OutlineCommand::Expand => self.expand(&rows[current]),
            OutlineCommand::ToggleFold => self.toggle_fold(&rows[current]),
            OutlineCommand::FoldAll => self.fold_all(items),
            OutlineCommand::UnfoldAll => self.unfold_all(),
        };
        self.refresh(items, viewport_height);
        outcome
    }

    /// Selects the item with `key`, unfolding its ancestors so that it is visible.
    ///
    /// Returns [`Outcome::Unchanged`] if no item has `key` or it is already selected and
    /// visible.
    pub fn select<I: OutlineItem<Key = K>>(
        &mut self,
        items: &[I],
        key: &K,
        viewport_height: usize,
    ) -> Outcome {
        let Some(index) = items.iter().position(|item| item.key() == key) else {
            return Outcome::Unchanged;
        };
        let revealed = self.unfold_ancestors(items, index);
        let moved = self.selected.as_ref() != Some(key);
        if moved {
            self.selected = Some(key.clone());
        }
        self.refresh(items, viewport_height);
        changed_if(revealed || moved)
    }

    /// Applies or suspends the folds; suspending them shows every item until they are applied
    /// again, which brings back the folds from before and drops those made meanwhile.
    ///
    /// Takes effect at the next operation, such as [`Outline::refresh`].
    pub fn set_folding(&mut self, folding: Folding) {
        match folding {
            Folding::Suspended => {
                if self.suspended.is_none() {
                    self.suspended = Some(std::mem::take(&mut self.collapsed));
                }
            }
            Folding::Applied => {
                if let Some(folds) = self.suspended.take() {
                    self.collapsed = folds;
                }
            }
        }
    }

    /// Selects the item of `shown` nearest to the selected item in `all`, after a filter
    /// changed which items of `all` are `shown`; `shown` must be a subsequence of `all`.
    ///
    /// The selection stays on its item if it is shown, unfolding its ancestors. Otherwise it
    /// moves to the shown item closest in document order, the following one on a tie.
    /// Without a selection, or if the selected item is not in `all`, this is a refresh.
    pub fn select_nearest<I: OutlineItem<Key = K>>(
        &mut self,
        all: &[I],
        shown: &[I],
        viewport_height: usize,
    ) {
        match self
            .selected
            .as_ref()
            .and_then(|selected| nearest(all, shown, selected))
            .cloned()
        {
            // Whether the selection moved does not matter to a filter.
            Some(key) => {
                let _: Outcome = self.select(shown, &key, viewport_height);
            }
            None => self.refresh(shown, viewport_height),
        }
    }

    /// Replaces the selected, the anchor and the folded keys for which `rename` returns a new key, after
    /// items changed their keys, so that they stay selected and folded.
    ///
    /// Keys for which `rename` returns `None` are kept; a selected key whose item was removed
    /// hands the selection to a neighbour at the next refresh.
    pub fn rename_keys(&mut self, mut rename: impl FnMut(&K) -> Option<K>) {
        for key in [&mut self.selected, &mut self.anchor] {
            if let Some(renamed) = key.as_ref().and_then(&mut rename) {
                *key = Some(renamed);
            }
        }
        for folds in std::iter::once(&mut self.collapsed).chain(self.suspended.as_mut()) {
            *folds = folds
                .drain()
                .map(|key| rename(&key).unwrap_or(key))
                .collect();
        }
    }

    fn refreshed_rows<'a, I: OutlineItem<Key = K>>(
        &mut self,
        items: &'a [I],
        viewport_height: usize,
    ) -> Vec<OutlineRow<'a, I>> {
        let rows = self.visible_rows(items);
        match self.resolve_selected_row(items, &rows) {
            Some(row) => {
                self.set_selection(&rows, row);
                self.scroll_to_selection(rows.len(), viewport_height);
            }
            None => {
                *self = Self {
                    collapsed: std::mem::take(&mut self.collapsed),
                    suspended: self.suspended.take(),
                    ..Self::default()
                }
            }
        }
        rows
    }

    fn resolve_selected_row<I: OutlineItem<Key = K>>(
        &self,
        items: &[I],
        rows: &[OutlineRow<'_, I>],
    ) -> Option<usize> {
        let last_row = rows.len().checked_sub(1)?;
        let Some(key) = &self.selected else {
            return Some(0);
        };
        if let Some(row) = rows.iter().position(|row| row.item.key() == key) {
            return Some(row);
        }
        let Some(index) = items.iter().position(|item| item.key() == key) else {
            return Some(self.selected_row.min(last_row));
        };
        // A hidden item is preceded by its folded ancestor and that ancestor's hidden
        // descendants only, so the last visible row before it is the ancestor.
        let rows_before = rows.partition_point(|row| row.index < index);
        Some(rows_before.saturating_sub(1))
    }

    fn set_selection<I: OutlineItem<Key = K>>(&mut self, rows: &[OutlineRow<'_, I>], row: usize) {
        let key = rows[row].item.key();
        if self.selected.as_ref() != Some(key) {
            self.selected = Some(key.clone());
        }
        self.selected_row = row;
    }

    fn scroll_to_selection(&mut self, row_count: usize, viewport_height: usize) {
        let height = viewport_height.max(1);
        let margin = SCROLL_MARGIN.min((height - 1) / 2);
        let row = self.selected_row;
        let lowest_offset = (row + margin + 1).saturating_sub(height);
        let highest_offset = row.saturating_sub(margin);
        let last_full_page = row_count.saturating_sub(height);
        self.offset = self
            .offset
            .clamp(lowest_offset, highest_offset)
            .min(last_full_page);
    }

    fn move_to<I: OutlineItem<Key = K>>(
        &mut self,
        rows: &[OutlineRow<'_, I>],
        target_row: usize,
    ) -> Outcome {
        let target_row = target_row.min(rows.len() - 1);
        if target_row == self.selected_row {
            return Outcome::Unchanged;
        }
        self.set_selection(rows, target_row);
        Outcome::Changed
    }

    fn move_to_parent<I: OutlineItem<Key = K>>(&mut self, rows: &[OutlineRow<'_, I>]) -> Outcome {
        let depth = rows[self.selected_row].item.depth();
        match rows[..self.selected_row]
            .iter()
            .rposition(|row| row.item.depth() < depth)
        {
            Some(parent_row) => self.move_to(rows, parent_row),
            None => Outcome::Unchanged,
        }
    }

    fn collapse<I: OutlineItem<Key = K>>(&mut self, rows: &[OutlineRow<'_, I>]) -> Outcome {
        let row = &rows[self.selected_row];
        match row.fold {
            Fold::Expanded => {
                self.collapsed.insert(row.item.key().clone());
                Outcome::Changed
            }
            Fold::Leaf | Fold::Collapsed => self.move_to_parent(rows),
        }
    }

    fn expand<I: OutlineItem<Key = K>>(&mut self, row: &OutlineRow<'_, I>) -> Outcome {
        match row.fold {
            Fold::Collapsed => changed_if(self.collapsed.remove(row.item.key())),
            Fold::Leaf | Fold::Expanded => Outcome::Unchanged,
        }
    }

    fn toggle_fold<I: OutlineItem<Key = K>>(&mut self, row: &OutlineRow<'_, I>) -> Outcome {
        match row.fold {
            Fold::Expanded => changed_if(self.collapsed.insert(row.item.key().clone())),
            Fold::Collapsed => changed_if(self.collapsed.remove(row.item.key())),
            Fold::Leaf => Outcome::Unchanged,
        }
    }

    fn fold_all<I: OutlineItem<Key = K>>(&mut self, items: &[I]) -> Outcome {
        let folded_before = self.collapsed.len();
        self.collapsed.extend(
            items
                .windows(2)
                .filter(|pair| pair[1].depth() > pair[0].depth())
                .map(|pair| pair[0].key().clone()),
        );
        changed_if(self.collapsed.len() != folded_before)
    }

    fn unfold_all(&mut self) -> Outcome {
        let had_folds = !self.collapsed.is_empty();
        self.collapsed.clear();
        changed_if(had_folds)
    }

    /// Unfolds every ancestor of `items[index]` and tells whether any was folded.
    fn unfold_ancestors<I: OutlineItem<Key = K>>(&mut self, items: &[I], index: usize) -> bool {
        let mut depth = items[index].depth();
        let mut unfolded = false;
        for item in items[..index].iter().rev() {
            if depth == 0 {
                break;
            }
            if item.depth() < depth {
                depth = item.depth();
                unfolded |= self.collapsed.remove(item.key());
            }
        }
        unfolded
    }
}

/// Keeps the items for which `is_shown` holds and leaves out every other item together with
/// its descendants, so that the result is again a tree in document order.
///
/// This is how a pane filters its items before it hands them to an [`Outline`]: the outline
/// then never sees the hidden items, and a selected item that is left out hands the selection
/// to the item now in its row. Filters compose by combining predicates with `&&` or by
/// chaining calls. A shown item below a hidden ancestor is left out with that ancestor.
///
/// # Examples
///
/// ```
/// use rudo::ui::outline::{OutlineItem, prune};
///
/// struct Entry {
///     name: &'static str,
///     depth: usize,
/// }
///
/// impl OutlineItem for Entry {
///     type Key = &'static str;
///
///     fn key(&self) -> &Self::Key {
///         &self.name
///     }
///
///     fn depth(&self) -> usize {
///         self.depth
///     }
/// }
///
/// let items = [
///     Entry { name: "groceries", depth: 0 },
///     Entry { name: "milk", depth: 1 },
///     Entry { name: "work", depth: 0 },
/// ];
/// let shown: Vec<&str> = prune(items, |entry| entry.name != "groceries")
///     .map(|entry| entry.name)
///     .collect();
/// assert_eq!(shown, ["work"]);
/// ```
pub fn prune<I: OutlineItem>(
    items: impl IntoIterator<Item = I>,
    mut is_shown: impl FnMut(&I) -> bool,
) -> impl Iterator<Item = I> {
    let mut hidden_depth = None;
    items.into_iter().filter(move |item| {
        let depth = item.depth();
        if hidden_depth.is_some_and(|hidden_depth| depth > hidden_depth) {
            return false;
        }
        let shown = is_shown(item);
        hidden_depth = (!shown).then_some(depth);
        shown
    })
}

/// Keeps the items for which `is_match` holds together with their ancestors, which are
/// shown as [`FilterRole::Context`], and leaves out every other item.
///
/// Unlike [`prune`], a match below an item that does not match is kept, so this is how a
/// search filter reduces the items. Combine it with [`prune`] by pruning first: a match below
/// an item that `prune` leaves out stays hidden. Runs in linear time.
///
/// # Examples
///
/// ```
/// use rudo::ui::outline::{FilterRole, OutlineItem, keep_matches};
///
/// struct Entry {
///     name: &'static str,
///     depth: usize,
/// }
///
/// impl OutlineItem for Entry {
///     type Key = &'static str;
///
///     fn key(&self) -> &Self::Key {
///         &self.name
///     }
///
///     fn depth(&self) -> usize {
///         self.depth
///     }
/// }
///
/// let items = [
///     Entry { name: "groceries", depth: 0 },
///     Entry { name: "milk", depth: 1 },
///     Entry { name: "work", depth: 0 },
/// ];
/// let shown: Vec<(&str, FilterRole)> = keep_matches(items, |entry| entry.name == "milk")
///     .map(|(entry, role)| (entry.name, role))
///     .collect();
/// assert_eq!(shown, [("groceries", FilterRole::Context), ("milk", FilterRole::Match)]);
/// ```
pub fn keep_matches<I: OutlineItem>(
    items: impl IntoIterator<Item = I>,
    mut is_match: impl FnMut(&I) -> bool,
) -> impl Iterator<Item = (I, FilterRole)> {
    let items: Vec<I> = items.into_iter().collect();
    let mut roles: Vec<Option<FilterRole>> = vec![None; items.len()];
    let mut ancestors: Vec<usize> = Vec::new();
    for (index, item) in items.iter().enumerate() {
        while ancestors
            .last()
            .is_some_and(|&ancestor| items[ancestor].depth() >= item.depth())
        {
            ancestors.pop();
        }
        if is_match(item) {
            roles[index] = Some(FilterRole::Match);
            // An ancestor with a role has its own ancestors marked already, so every item is
            // marked at most once.
            for &ancestor in ancestors.iter().rev() {
                if roles[ancestor].is_some() {
                    break;
                }
                roles[ancestor] = Some(FilterRole::Context);
            }
        }
        ancestors.push(index);
    }
    items
        .into_iter()
        .zip(roles)
        .filter_map(|(item, role)| role.map(|role| (item, role)))
}

/// Returns the key of the item of `shown` nearest to the item with `key` in `all`, the
/// following one on a tie, or `None` if `key` is not in `all` or nothing is shown.
fn nearest<'a, I: OutlineItem>(all: &[I], shown: &'a [I], key: &I::Key) -> Option<&'a I::Key> {
    let target = all.iter().position(|item| item.key() == key)?;
    let mut shown_items = shown.iter().peekable();
    let mut before: Option<(usize, &'a I)> = None;
    for (index, item) in all.iter().enumerate() {
        let Some(&next) = shown_items.peek() else {
            break;
        };
        if next.key() != item.key() {
            continue;
        }
        shown_items.next();
        if index >= target {
            let closer_before =
                before.filter(|&(before_index, _)| target - before_index < index - target);
            return Some(closer_before.map_or(next, |(_, item)| item).key());
        }
        before = Some((index, next));
    }
    before.map(|(_, item)| item.key())
}

/// Computes the rows of `items` that no folded ancestor hides; items left out by a filter
/// are removed beforehand with [`prune`].
fn visible_rows<'a, I: OutlineItem>(
    items: &'a [I],
    collapsed: &HashSet<I::Key>,
) -> Vec<OutlineRow<'a, I>> {
    let mut rows = Vec::with_capacity(items.len());
    let mut folded_depth = None;
    for (index, item) in items.iter().enumerate() {
        let depth = item.depth();
        if folded_depth.is_some_and(|folded_depth| depth > folded_depth) {
            continue;
        }
        let has_children = items
            .get(index + 1)
            .is_some_and(|next| next.depth() > depth);
        let fold = match (has_children, collapsed.contains(item.key())) {
            (false, _) => Fold::Leaf,
            (true, false) => Fold::Expanded,
            (true, true) => Fold::Collapsed,
        };
        folded_depth = (fold == Fold::Collapsed).then_some(depth);
        rows.push(OutlineRow { index, item, fold });
    }
    rows
}

/// Rows moved by a half page command; at least one so that tiny viewports still move.
fn half_page(viewport_height: usize) -> usize {
    (viewport_height / 2).max(1)
}

fn changed_if(changed: bool) -> Outcome {
    if changed {
        Outcome::Changed
    } else {
        Outcome::Unchanged
    }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;
    use rstest::rstest;
    use strum::IntoEnumIterator;

    use super::*;

    const HEIGHT: usize = 10;

    #[derive(Debug)]
    struct Node {
        key: char,
        depth: usize,
    }

    impl OutlineItem for Node {
        type Key = char;

        fn key(&self) -> &char {
            &self.key
        }

        fn depth(&self) -> usize {
            self.depth
        }
    }

    /// The observable state of an outline after an operation.
    #[derive(Debug, PartialEq)]
    struct Observed {
        outcome: Outcome,
        selected: Option<char>,
        visible: String,
        offset: usize,
    }

    impl Observed {
        fn of(outline: &Outline<char>, items: &[Node], outcome: Outcome) -> Self {
            Self {
                outcome,
                selected: outline.selected().copied(),
                visible: outline
                    .visible_rows(items)
                    .iter()
                    .map(|row| row.item().key)
                    .collect(),
                offset: outline.offset(),
            }
        }

        fn selection_is_inside_viewport(&self, viewport_height: usize) -> bool {
            let Some(selected) = self.selected else {
                return self.visible.is_empty();
            };
            self.visible
                .chars()
                .position(|key| key == selected)
                .is_some_and(|row| (self.offset..self.offset + viewport_height).contains(&row))
        }
    }

    /// Builds nodes from `"key:depth"` pairs separated by spaces.
    fn nodes(spec: &str) -> Vec<Node> {
        spec.split_whitespace()
            .map(|node| {
                let (key, depth) = node.split_once(':').unwrap();
                Node {
                    key: key.chars().next().unwrap(),
                    depth: depth.parse().unwrap(),
                }
            })
            .collect()
    }

    /// The tree a(b(c), d), e(f), g.
    fn tree() -> Vec<Node> {
        nodes("a:0 b:1 c:2 d:1 e:0 f:1 g:0")
    }

    /// Twenty top-level items `a` to `t`.
    fn flat_list() -> Vec<Node> {
        ('a'..='t').map(|key| Node { key, depth: 0 }).collect()
    }

    fn outline(selected: char, collapsed: &str, items: &[Node], height: usize) -> Outline<char> {
        let mut outline = Outline {
            selected: Some(selected),
            selected_row: 0,
            offset: 0,
            collapsed: collapsed.chars().collect(),
            suspended: None,
            anchor: None,
        };
        outline.refresh(items, height);
        outline
    }

    fn apply(
        mut outline: Outline<char>,
        items: &[Node],
        command: OutlineCommand,
        height: usize,
    ) -> Observed {
        let outcome = outline.apply(items, command, height);
        Observed::of(&outline, items, outcome)
    }

    #[rstest]
    #[case::everything_shown("", "abcdefg")]
    #[case::leaf("c", "abdefg")]
    #[case::subtree("b", "adefg")]
    #[case::root_with_descendants("a", "efg")]
    #[case::shown_child_of_hidden_parent("e", "abcdg")]
    #[case::siblings("bd", "aefg")]
    #[case::everything("aeg", "")]
    fn prune_leaves_out_hidden_items_with_their_descendants(
        #[case] hidden: &str,
        #[case] expected: &str,
    ) {
        let items = tree();

        let system_under_test: String = prune(items, |node| !hidden.contains(node.key))
            .map(|node| node.key)
            .collect();

        assert_eq!(system_under_test, expected);
    }

    /// Writes the kept items of `kept` as their keys, context items in upper case.
    fn roles(kept: impl Iterator<Item = (Node, FilterRole)>) -> String {
        kept.map(|(node, role)| match role {
            FilterRole::Match => node.key,
            FilterRole::Context => node.key.to_ascii_uppercase(),
        })
        .collect()
    }

    #[rstest]
    #[case::nothing("", "")]
    #[case::root("a", "a")]
    #[case::leaf_with_its_ancestors("c", "ABc")]
    #[case::child_of_a_parent_that_does_not_match("f", "Ef")]
    #[case::match_inside_a_match("ac", "aBc")]
    #[case::siblings_share_their_ancestors("cd", "ABcd")]
    #[case::everything("abcdefg", "abcdefg")]
    fn keep_matches_keeps_ancestors_as_context(#[case] matching: &str, #[case] expected: &str) {
        let items = tree();

        let system_under_test = roles(keep_matches(items, |node| matching.contains(node.key)));

        assert_eq!(system_under_test, expected);
    }

    #[test]
    fn keep_matches_after_prune_hides_matches_below_pruned_items() {
        let items = tree();
        let expected = "Ef";

        let system_under_test = roles(keep_matches(prune(items, |node| node.key != 'b'), |node| {
            "cf".contains(node.key)
        }));

        assert_eq!(system_under_test, expected);
    }

    #[rstest]
    #[case::shown_item_stays_selected('c', "abcg", 'c')]
    #[case::following_on_a_tie('c', "bd", 'd')]
    #[case::closer_preceding('d', "bg", 'b')]
    #[case::closer_following('b', "ae", 'a')]
    #[case::only_preceding('g', "ab", 'b')]
    #[case::only_following('a', "fg", 'f')]
    fn select_nearest_moves_to_the_closest_shown_item(
        #[case] selected: char,
        #[case] shown: &str,
        #[case] expected: char,
    ) {
        let all = tree();
        let shown: Vec<Node> = tree()
            .into_iter()
            .filter(|node| shown.contains(node.key))
            .collect();
        let mut outline = outline(selected, "", &all, HEIGHT);

        let system_under_test = {
            outline.select_nearest(&all, &shown, HEIGHT);
            outline.selected().copied()
        };

        assert_eq!(system_under_test, Some(expected));
    }

    #[test]
    fn select_nearest_without_shown_items_clears_the_selection() {
        let all = tree();
        let mut outline = outline('c', "", &all, HEIGHT);

        let system_under_test = {
            outline.select_nearest(&all, &[], HEIGHT);
            outline.selected().copied()
        };

        assert_eq!(system_under_test, None);
    }

    #[test]
    fn suspended_folds_show_every_row() {
        let items = tree();
        let mut outline = outline('a', "a", &items, HEIGHT);
        let expected = "abcdefg";

        let system_under_test = {
            outline.set_folding(Folding::Suspended);
            outline.refresh(&items, HEIGHT);
            Observed::of(&outline, &items, Outcome::Unchanged).visible
        };

        assert_eq!(system_under_test, expected);
    }

    #[test]
    fn applied_folds_come_back_and_drop_those_made_meanwhile() {
        let items = tree();
        let mut outline = outline('a', "b", &items, HEIGHT);
        outline.set_folding(Folding::Suspended);
        outline.set_folding(Folding::Suspended);
        let _: Outcome = outline.apply(&items, OutlineCommand::FoldAll, HEIGHT);
        let expected = "abdefg";

        let system_under_test = {
            outline.set_folding(Folding::Applied);
            outline.set_folding(Folding::Applied);
            outline.refresh(&items, HEIGHT);
            Observed::of(&outline, &items, Outcome::Unchanged).visible
        };

        assert_eq!(system_under_test, expected);
    }

    #[test]
    fn renaming_keys_renames_suspended_folds() {
        let renamed = nodes("x:0 b:1 c:2 d:1 e:0 f:1 g:0");
        let mut outline = outline('g', "a", &tree(), HEIGHT);
        outline.set_folding(Folding::Suspended);
        outline.rename_keys(|&key| (key == 'a').then_some('x'));
        let expected = "xefg";

        let system_under_test = {
            outline.set_folding(Folding::Applied);
            outline.refresh(&renamed, HEIGHT);
            Observed::of(&outline, &renamed, Outcome::Unchanged).visible
        };

        assert_eq!(system_under_test, expected);
    }

    #[rstest]
    #[case::move_down(Action::MoveDown, Some(OutlineCommand::Down))]
    #[case::move_up(Action::MoveUp, Some(OutlineCommand::Up))]
    #[case::move_to_top(Action::MoveToTop, Some(OutlineCommand::Top))]
    #[case::move_to_bottom(Action::MoveToBottom, Some(OutlineCommand::Bottom))]
    #[case::half_page_down(Action::HalfPageDown, Some(OutlineCommand::HalfPageDown))]
    #[case::half_page_up(Action::HalfPageUp, Some(OutlineCommand::HalfPageUp))]
    #[case::collapse(Action::Collapse, Some(OutlineCommand::Collapse))]
    #[case::expand(Action::Expand, Some(OutlineCommand::Expand))]
    #[case::toggle_fold(Action::ToggleFold, Some(OutlineCommand::ToggleFold))]
    #[case::fold_all(Action::FoldAll, Some(OutlineCommand::FoldAll))]
    #[case::unfold_all(Action::UnfoldAll, Some(OutlineCommand::UnfoldAll))]
    #[case::open(Action::Open, None)]
    #[case::delete(Action::Delete, None)]
    #[case::quit(Action::Quit, None)]
    fn actions_map_to_outline_commands(
        #[case] action: Action,
        #[case] expected: Option<OutlineCommand>,
    ) {
        let system_under_test = OutlineCommand::from_action(action);

        assert_eq!(system_under_test, expected);
    }

    #[test]
    fn every_command_but_parent_has_an_action() {
        let expected: HashSet<OutlineCommand> = OutlineCommand::iter()
            .filter(|command| *command != OutlineCommand::Parent)
            .collect();

        let system_under_test: HashSet<OutlineCommand> = Action::iter()
            .filter_map(OutlineCommand::from_action)
            .collect();

        assert_eq!(system_under_test, expected);
    }

    #[rstest]
    #[case::down(OutlineCommand::Down, 'a', "", 'b', Outcome::Changed)]
    #[case::down_at_bottom(OutlineCommand::Down, 'g', "", 'g', Outcome::Unchanged)]
    #[case::down_skips_folded(OutlineCommand::Down, 'a', "a", 'e', Outcome::Changed)]
    #[case::up(OutlineCommand::Up, 'b', "", 'a', Outcome::Changed)]
    #[case::up_at_top(OutlineCommand::Up, 'a', "", 'a', Outcome::Unchanged)]
    #[case::up_skips_folded(OutlineCommand::Up, 'e', "a", 'a', Outcome::Changed)]
    #[case::top(OutlineCommand::Top, 'f', "", 'a', Outcome::Changed)]
    #[case::top_at_top(OutlineCommand::Top, 'a', "", 'a', Outcome::Unchanged)]
    #[case::bottom(OutlineCommand::Bottom, 'a', "", 'g', Outcome::Changed)]
    #[case::bottom_at_bottom(OutlineCommand::Bottom, 'g', "e", 'g', Outcome::Unchanged)]
    #[case::bottom_skips_folded(OutlineCommand::Bottom, 'a', "e", 'g', Outcome::Changed)]
    #[case::parent(OutlineCommand::Parent, 'c', "", 'b', Outcome::Changed)]
    #[case::parent_of_sibling(OutlineCommand::Parent, 'd', "", 'a', Outcome::Changed)]
    #[case::parent_of_top_level(OutlineCommand::Parent, 'e', "", 'e', Outcome::Unchanged)]
    fn moves_selection(
        #[case] command: OutlineCommand,
        #[case] selected: char,
        #[case] collapsed: &str,
        #[case] expected_selected: char,
        #[case] expected_outcome: Outcome,
    ) {
        let items = tree();
        let outline = outline(selected, collapsed, &items, HEIGHT);

        let system_under_test = apply(outline, &items, command, HEIGHT);

        assert_eq!(system_under_test.selected, Some(expected_selected));
        assert_eq!(system_under_test.outcome, expected_outcome);
    }

    #[rstest]
    #[case::down(OutlineCommand::HalfPageDown, 'a', 4, 'c', Outcome::Changed)]
    #[case::down_clamps(OutlineCommand::HalfPageDown, 'f', 4, 'g', Outcome::Changed)]
    #[case::down_at_bottom(OutlineCommand::HalfPageDown, 'g', 4, 'g', Outcome::Unchanged)]
    #[case::up(OutlineCommand::HalfPageUp, 'g', 4, 'e', Outcome::Changed)]
    #[case::up_clamps(OutlineCommand::HalfPageUp, 'b', 4, 'a', Outcome::Changed)]
    #[case::up_at_top(OutlineCommand::HalfPageUp, 'a', 4, 'a', Outcome::Unchanged)]
    #[case::down_tiny_viewport(OutlineCommand::HalfPageDown, 'a', 1, 'b', Outcome::Changed)]
    #[case::down_empty_viewport(OutlineCommand::HalfPageDown, 'a', 0, 'b', Outcome::Changed)]
    fn moves_half_a_page(
        #[case] command: OutlineCommand,
        #[case] selected: char,
        #[case] height: usize,
        #[case] expected_selected: char,
        #[case] expected_outcome: Outcome,
    ) {
        let items = tree();
        let outline = outline(selected, "", &items, height);

        let system_under_test = apply(outline, &items, command, height);

        assert_eq!(system_under_test.selected, Some(expected_selected));
        assert_eq!(system_under_test.outcome, expected_outcome);
    }

    #[rstest]
    #[case::collapse_folds(OutlineCommand::Collapse, 'a', "", 'a', "aefg", Outcome::Changed)]
    #[case::collapse_leaf(OutlineCommand::Collapse, 'c', "", 'b', "abcdefg", Outcome::Changed)]
    #[case::collapse_folded(OutlineCommand::Collapse, 'b', "b", 'a', "abdefg", Outcome::Changed)]
    #[case::collapse_root_leaf(
        OutlineCommand::Collapse,
        'g',
        "",
        'g',
        "abcdefg",
        Outcome::Unchanged
    )]
    #[case::collapse_root_folded(
        OutlineCommand::Collapse,
        'a',
        "a",
        'a',
        "aefg",
        Outcome::Unchanged
    )]
    #[case::expand(OutlineCommand::Expand, 'a', "a", 'a', "abcdefg", Outcome::Changed)]
    #[case::expand_expanded(OutlineCommand::Expand, 'a', "", 'a', "abcdefg", Outcome::Unchanged)]
    #[case::expand_leaf(OutlineCommand::Expand, 'c', "", 'c', "abcdefg", Outcome::Unchanged)]
    #[case::toggle_folds(OutlineCommand::ToggleFold, 'e', "", 'e', "abcdeg", Outcome::Changed)]
    #[case::toggle_unfolds(OutlineCommand::ToggleFold, 'e', "e", 'e', "abcdefg", Outcome::Changed)]
    #[case::toggle_leaf(
        OutlineCommand::ToggleFold,
        'g',
        "",
        'g',
        "abcdefg",
        Outcome::Unchanged
    )]
    #[case::fold_all(OutlineCommand::FoldAll, 'c', "", 'a', "aeg", Outcome::Changed)]
    #[case::fold_all_folded(OutlineCommand::FoldAll, 'a', "abe", 'a', "aeg", Outcome::Unchanged)]
    #[case::unfold_all(
        OutlineCommand::UnfoldAll,
        'a',
        "abe",
        'a',
        "abcdefg",
        Outcome::Changed
    )]
    #[case::unfold_all_unfolded(
        OutlineCommand::UnfoldAll,
        'a',
        "",
        'a',
        "abcdefg",
        Outcome::Unchanged
    )]
    fn folds(
        #[case] command: OutlineCommand,
        #[case] selected: char,
        #[case] collapsed: &str,
        #[case] expected_selected: char,
        #[case] expected_visible: &str,
        #[case] expected_outcome: Outcome,
    ) {
        let items = tree();
        let outline = outline(selected, collapsed, &items, HEIGHT);

        let system_under_test = apply(outline, &items, command, HEIGHT);

        assert_eq!(system_under_test.selected, Some(expected_selected));
        assert_eq!(system_under_test.visible, expected_visible);
        assert_eq!(system_under_test.outcome, expected_outcome);
    }

    #[rstest]
    #[case::leaf('c', Fold::Leaf)]
    #[case::expanded('b', Fold::Expanded)]
    #[case::collapsed('e', Fold::Collapsed)]
    fn rows_know_their_fold_state(#[case] key: char, #[case] expected: Fold) {
        let items = tree();
        let outline = outline('a', "e", &items, HEIGHT);

        let system_under_test = outline.visible_rows(&items);

        assert_eq!(
            system_under_test
                .iter()
                .find(|row| row.item().key == key)
                .map(OutlineRow::fold),
            Some(expected)
        );
    }

    #[test]
    fn every_fold_state_has_a_case() {
        let items = tree();
        let expected: Vec<Fold> = Fold::iter().collect();

        let system_under_test = outline('a', "e", &items, HEIGHT);

        assert!(expected.iter().all(|fold| {
            system_under_test
                .visible_rows(&items)
                .iter()
                .any(|row| row.fold() == *fold)
        }));
    }

    #[rstest]
    #[case::down_within_margin(6, 0, OutlineCommand::Down, 10, 0)]
    #[case::down_past_margin(7, 0, OutlineCommand::Down, 10, 1)]
    #[case::up_past_margin(12, 10, OutlineCommand::Up, 10, 9)]
    #[case::bottom(0, 0, OutlineCommand::Bottom, 10, 10)]
    #[case::top(19, 10, OutlineCommand::Top, 10, 0)]
    #[case::half_page_down_keeps_screen_position(2, 0, OutlineCommand::HalfPageDown, 10, 5)]
    #[case::half_page_up_keeps_screen_position(17, 10, OutlineCommand::HalfPageUp, 10, 5)]
    #[case::reduced_margin(1, 0, OutlineCommand::Down, 3, 1)]
    #[case::no_margin_in_two_rows(0, 0, OutlineCommand::Down, 2, 0)]
    #[case::single_row(0, 0, OutlineCommand::Down, 1, 1)]
    #[case::zero_rows(0, 0, OutlineCommand::Down, 0, 1)]
    fn scrolls_to_keep_selection_visible(
        #[case] selected_row: usize,
        #[case] offset: usize,
        #[case] command: OutlineCommand,
        #[case] height: usize,
        #[case] expected_offset: usize,
    ) {
        let items = flat_list();
        let mut outline = outline(items[selected_row].key, "", &items, height);
        outline.offset = offset;

        let system_under_test = apply(outline, &items, command, height);

        assert_eq!(system_under_test.offset, expected_offset);
    }

    #[rstest]
    #[case::selects_first_row("a:0 b:1", None, Some('a'))]
    #[case::removed_hands_over_to_next(
        "a:0 b:1 d:1 e:0 f:1 g:0",
        Some(('c', 2)),
        Some('d')
    )]
    #[case::removed_last_hands_over_to_previous("a:0 b:1 c:2", Some(('d', 3)), Some('c'))]
    #[case::empty_has_no_selection("", Some(('a', 0)), None)]
    fn refresh_keeps_a_valid_selection(
        #[case] spec: &str,
        #[case] previous_selection: Option<(char, usize)>,
        #[case] expected: Option<char>,
    ) {
        let items = nodes(spec);
        let mut outline = Outline::new();
        outline.selected = previous_selection.map(|(key, _)| key);
        outline.selected_row = previous_selection.map_or(0, |(_, row)| row);

        let system_under_test = {
            outline.refresh(&items, HEIGHT);
            outline
        };

        assert_eq!(system_under_test.selected(), expected.as_ref());
    }

    #[test]
    fn empty_outline_ignores_every_command() {
        let items: Vec<Node> = Vec::new();
        let expected = Observed {
            outcome: Outcome::Unchanged,
            selected: None,
            visible: String::new(),
            offset: 0,
        };

        let system_under_test: Vec<Observed> = OutlineCommand::iter()
            .map(|command| apply(Outline::new(), &items, command, HEIGHT))
            .collect();

        assert!(
            system_under_test
                .iter()
                .all(|observed| *observed == expected)
        );
    }

    #[rstest]
    #[case::other('f', "", 'f', "abcdefg", Outcome::Changed)]
    #[case::current('a', "", 'a', "abcdefg", Outcome::Unchanged)]
    #[case::hidden_unfolds_ancestors('c', "abe", 'c', "abcdeg", Outcome::Changed)]
    #[case::unknown('z', "", 'a', "abcdefg", Outcome::Unchanged)]
    fn selects_by_key(
        #[case] key: char,
        #[case] collapsed: &str,
        #[case] expected_selected: char,
        #[case] expected_visible: &str,
        #[case] expected_outcome: Outcome,
    ) {
        let items = tree();
        let mut outline = outline('a', collapsed, &items, HEIGHT);

        let system_under_test = {
            let outcome = outline.select(&items, &key, HEIGHT);
            Observed::of(&outline, &items, outcome)
        };

        assert_eq!(system_under_test.selected, Some(expected_selected));
        assert_eq!(system_under_test.visible, expected_visible);
        assert_eq!(system_under_test.outcome, expected_outcome);
    }

    #[test]
    fn renamed_keys_stay_selected_and_folded() {
        let renamed = nodes("x:0 b:1 c:2 d:1 y:0 f:1 g:0");
        let mut outline = outline('a', "ae", &tree(), HEIGHT);
        outline.rename_keys(|key| match key {
            'a' => Some('x'),
            'e' => Some('y'),
            _ => None,
        });
        let expected = Observed {
            outcome: Outcome::Unchanged,
            selected: Some('x'),
            visible: "xyg".to_owned(),
            offset: 0,
        };

        let system_under_test = {
            outline.refresh(&renamed, HEIGHT);
            Observed::of(&outline, &renamed, Outcome::Unchanged)
        };

        assert_eq!(system_under_test, expected);
    }

    /// Returns the keys of the visual selection of `outline` among the visible rows of
    /// `items`, or `None` without one.
    fn selected_keys(outline: &Outline<char>, items: &[Node]) -> Option<String> {
        let rows = outline.visible_rows(items);
        let range = outline.selection_rows(&rows)?;
        Some(rows[range].iter().map(|row| row.item().key).collect())
    }

    #[rstest]
    #[case::only_the_anchor('b', "", &[], "b")]
    #[case::downwards('b', "", &[OutlineCommand::Down, OutlineCommand::Down], "bcd")]
    #[case::upwards('d', "", &[OutlineCommand::Up, OutlineCommand::Up], "bcd")]
    #[case::back_across_the_anchor(
        'c',
        "",
        &[OutlineCommand::Down, OutlineCommand::Up, OutlineCommand::Up],
        "bc"
    )]
    #[case::to_the_bottom('d', "", &[OutlineCommand::Bottom], "defg")]
    #[case::over_a_fold('a', "b", &[OutlineCommand::Down, OutlineCommand::Down], "abd")]
    fn selection_spans_the_visible_rows_from_the_anchor_to_the_cursor(
        #[case] anchor: char,
        #[case] collapsed: &str,
        #[case] commands: &[OutlineCommand],
        #[case] expected: &str,
    ) {
        let items = tree();
        let mut outline = outline(anchor, collapsed, &items, HEIGHT);
        outline.start_selection();
        for &command in commands {
            let _: Outcome = outline.apply(&items, command, HEIGHT);
        }

        let system_under_test = selected_keys(&outline, &items);

        assert_eq!(system_under_test.as_deref(), Some(expected));
    }

    #[test]
    fn outline_without_selection_start_has_no_selection() {
        let items = tree();

        let system_under_test = outline('b', "", &items, HEIGHT);

        assert_eq!(system_under_test.anchor(), None);
        assert_eq!(selected_keys(&system_under_test, &items), None);
    }

    #[test]
    fn ended_selection_is_gone() {
        let items = tree();
        let mut outline = outline('b', "", &items, HEIGHT);
        outline.start_selection();
        outline.end_selection();

        let system_under_test = selected_keys(&outline, &items);

        assert_eq!(system_under_test, None);
        assert_eq!(outline.anchor(), None);
    }

    #[test]
    fn removed_anchor_ends_the_selection() {
        let items = tree();
        let without_b = nodes("a:0 d:1 e:0 f:1 g:0");
        let mut outline = outline('b', "", &items, HEIGHT);
        outline.start_selection();
        let _: Outcome = outline.apply(&items, OutlineCommand::Bottom, HEIGHT);
        outline.refresh(&without_b, HEIGHT);

        let system_under_test = selected_keys(&outline, &without_b);

        assert_eq!(system_under_test, None);
        assert_eq!(outline.anchor(), Some(&'b'));
    }

    #[test]
    fn renamed_anchor_keeps_the_selection() {
        let renamed = nodes("a:0 x:1 c:2 d:1 e:0 f:1 g:0");
        let mut outline = outline('b', "", &tree(), HEIGHT);
        outline.start_selection();
        let _: Outcome = outline.apply(&tree(), OutlineCommand::Down, HEIGHT);
        outline.rename_keys(|key| (*key == 'b').then_some('x'));
        outline.refresh(&renamed, HEIGHT);

        let system_under_test = selected_keys(&outline, &renamed);

        assert_eq!(system_under_test.as_deref(), Some("xc"));
    }

    #[test]
    fn empty_outline_starts_no_selection() {
        let items: Vec<Node> = Vec::new();
        let mut outline = Outline::new();
        outline.refresh(&items, HEIGHT);
        outline.start_selection();

        let system_under_test = outline.anchor();

        assert_eq!(system_under_test, None);
    }

    /// A change a user or the file system can make between two frames.
    #[derive(Debug, Clone)]
    enum Step {
        Command(OutlineCommand),
        Select(usize),
        Remove(usize),
    }

    fn step() -> impl Strategy<Value = Step> {
        prop_oneof![
            proptest::sample::select(OutlineCommand::iter().collect::<Vec<_>>())
                .prop_map(Step::Command),
            any::<usize>().prop_map(Step::Select),
            any::<usize>().prop_map(Step::Remove),
        ]
    }

    fn run(depths: &[usize], steps: &[Step], height: usize) -> Observed {
        let mut items: Vec<Node> = ('a'..='z')
            .zip(depths)
            .map(|(key, &depth)| Node { key, depth })
            .collect();
        let mut outline = Outline::new();
        outline.refresh(&items, height);
        let mut outcome = Outcome::Unchanged;
        for step in steps {
            outcome = match *step {
                Step::Command(command) => outline.apply(&items, command, height),
                Step::Select(index) if !items.is_empty() => {
                    let key = items[index % items.len()].key;
                    outline.select(&items, &key, height)
                }
                Step::Remove(index) if !items.is_empty() => {
                    let index = index % items.len();
                    items.remove(index);
                    outline.refresh(&items, height);
                    Outcome::Changed
                }
                Step::Select(_) | Step::Remove(_) => Outcome::Unchanged,
            };
        }
        Observed::of(&outline, &items, outcome)
    }

    /// Runs `steps` after starting a selection at row `anchor` and returns the visible keys,
    /// the selected key and the keys of the selection.
    fn run_selection(
        depths: &[usize],
        anchor: usize,
        steps: &[OutlineCommand],
        height: usize,
    ) -> (String, Option<char>, Option<char>, Option<String>) {
        let items: Vec<Node> = ('a'..='z')
            .zip(depths)
            .map(|(key, &depth)| Node { key, depth })
            .collect();
        let mut outline = Outline::new();
        outline.refresh(&items, height);
        if !items.is_empty() {
            let key = items[anchor % items.len()].key;
            let _: Outcome = outline.select(&items, &key, height);
        }
        outline.start_selection();
        for &command in steps {
            let _: Outcome = outline.apply(&items, command, height);
        }
        let visible: String = outline
            .visible_rows(&items)
            .iter()
            .map(|row| row.item().key)
            .collect();
        (
            visible,
            outline.selected().copied(),
            outline.anchor().copied(),
            selected_keys(&outline, &items),
        )
    }

    proptest! {
        #[test]
        fn selection_is_a_contiguous_run_of_visible_rows_with_anchor_and_cursor(
            depths in proptest::collection::vec(0usize..4, 1..26),
            anchor in any::<usize>(),
            steps in proptest::collection::vec(
                proptest::sample::select(OutlineCommand::iter().collect::<Vec<_>>()),
                0..40,
            ),
            height in 1usize..12,
        ) {
            let (visible, cursor, anchor, system_under_test) =
                run_selection(&depths, anchor, &steps, height);

            let anchor_visible = anchor.is_some_and(|anchor| visible.contains(anchor));
            prop_assert_eq!(system_under_test.is_some(), anchor_visible);
            if let (Some(selection), Some(cursor), Some(anchor)) = (system_under_test, cursor, anchor) {
                prop_assert!(visible.contains(selection.as_str()));
                prop_assert!(selection.contains(cursor));
                prop_assert!(selection.contains(anchor));
                let ends = [selection.chars().next(), selection.chars().last()];
                prop_assert!(ends.contains(&Some(cursor)) && ends.contains(&Some(anchor)));
            }
        }
    }

    proptest! {
        #[test]
        fn selection_is_a_visible_row_inside_the_viewport(
            depths in proptest::collection::vec(0usize..4, 0..26),
            steps in proptest::collection::vec(step(), 0..40),
            height in 1usize..12,
        ) {
            let system_under_test = run(&depths, &steps, height);

            prop_assert!(system_under_test.selection_is_inside_viewport(height));
        }
    }
}

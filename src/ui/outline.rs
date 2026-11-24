//! Tree navigation shared by every tree-shaped pane: selection, folding and scrolling.
//!
//! The lists pane, the tasks pane and the help screen all show a flat sequence of rows that
//! encodes a tree through the depth of each row. [`Outline`] holds the navigation state of such
//! a pane and changes it through [`OutlineCommand`]s. It never owns the items: every operation
//! takes the current items, so edits, filtering and reloads cannot leave a stale copy behind.
//!
//! The selection is remembered by key instead of by row index, so it stays on the same entry
//! while rows above it are inserted, removed or folded away.

use std::collections::HashSet;
use std::hash::Hash;

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
            | Action::FocusNext
            | Action::FocusPrevious
            | Action::FocusLists
            | Action::FocusTasks
            | Action::Search
            | Action::ClearFilter
            | Action::Undo
            | Action::Redo
            | Action::ToggleCompleted
            | Action::OpenInEditor
            | Action::Open
            | Action::NewList
            | Action::NewFolder
            | Action::Rename
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
}

impl<K> Default for Outline<K> {
    fn default() -> Self {
        Self {
            selected: None,
            selected_row: 0,
            offset: 0,
            collapsed: HashSet::new(),
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

    /// Replaces the selected and the folded keys for which `rename` returns a new key, after
    /// items changed their keys, so that they stay selected and folded.
    ///
    /// Keys for which `rename` returns `None` are kept; a selected key whose item was removed
    /// hands the selection to a neighbour at the next refresh.
    pub fn rename_keys(&mut self, mut rename: impl FnMut(&K) -> Option<K>) {
        if let Some(renamed) = self.selected.as_ref().and_then(&mut rename) {
            self.selected = Some(renamed);
        }
        self.collapsed = self
            .collapsed
            .drain()
            .map(|key| rename(&key).unwrap_or(key))
            .collect();
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

/// Computes the rows of `items` that no folded ancestor hides.
///
/// This is the single place that decides visibility, so a later filter predicate belongs here.
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

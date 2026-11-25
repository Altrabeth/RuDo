//! The state of the lists pane: navigation in the workspace tree and the active list.
//!
//! [`ListsPane`] never copies the workspace. [`tree_items`] flattens the current
//! [`Workspace`] in the order of the pane whenever the pane needs it, so lists that are created,
//! renamed or reloaded can never leave a stale row behind, and a sorted pane follows every
//! change of a list. While the bin is not empty, the tree ends with a row
//! for the bin, which opens the [`BinView`].

use std::path::{Path, PathBuf};

use crate::app::bin::BinView;
use crate::app::confirm::ConfirmKind;
use crate::app::entry_visibility::HiddenEntries;
use crate::app::folder_picker::FolderPurpose;
use crate::app::lists_order::{OrderedEntry, ordered_entries};
use crate::app::prompt::PromptKind;
use crate::app::query::Query;
use crate::app::search::{PaneFilter, SearchItem};
use crate::app::state::{App, Focus, Mode};
use crate::app::status::StatusMessage;
use crate::app::task_delete::counted_tasks;
use crate::config::ListOrder;
use crate::config::keymap::action::Action;
use crate::markdown::task_line::TaskLine;
use crate::paths::portable;
use crate::storage::bin::BinEntry;
use crate::storage::workspace::{Entry, Folder, List, ListPath, Workspace, moved_path};
use crate::ui::outline::{FilterRole, Outcome, Outline, OutlineCommand, OutlineItem, prune};
use crate::ui::text_input::TextInput;

/// Identifies a row of the workspace tree across rescans.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum EntryKey {
    /// A folder, by its path relative to the workspace root.
    Folder(PathBuf),
    /// A list, by its path.
    List(ListPath),
    /// The row of the bin at the end of the tree, by the path of the bin relative to the
    /// workspace root; it is no entry of the workspace, so nothing but opening acts on it.
    Bin(PathBuf),
}

impl EntryKey {
    /// Returns the path of the folder, list or bin relative to the workspace root.
    #[must_use]
    pub fn path(&self) -> &Path {
        match self {
            Self::Folder(path) | Self::Bin(path) => path,
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
            EntryKey::Bin(_) => None,
        }
    }
}

/// What a row of the workspace tree shows.
#[derive(Debug, Clone, Copy)]
pub enum TreeRow<'workspace> {
    /// A folder or list of the workspace.
    Entry(&'workspace Entry),
    /// The bin with the entries in it, after the tree while it is not empty.
    Bin(&'workspace [BinEntry]),
}

/// A row of the workspace tree with its key, nesting depth and why the filter of the lists
/// pane shows it.
#[derive(Debug)]
pub struct TreeItem<'workspace> {
    row: TreeRow<'workspace>,
    key: EntryKey,
    depth: usize,
    role: FilterRole,
}

impl<'workspace> TreeItem<'workspace> {
    /// Returns the row of `entry`, nested `depth` levels deep.
    fn new(entry: &'workspace Entry, depth: usize) -> Self {
        Self {
            row: TreeRow::Entry(entry),
            key: EntryKey::from(entry),
            depth,
            role: FilterRole::Match,
        }
    }

    /// Returns what this row shows.
    #[must_use]
    pub fn row(&self) -> TreeRow<'workspace> {
        self.row
    }

    /// Returns the folder or list of this row, or `None` for the row of the bin.
    #[must_use]
    pub fn entry(&self) -> Option<&'workspace Entry> {
        match self.row {
            TreeRow::Entry(entry) => Some(entry),
            TreeRow::Bin(_) => None,
        }
    }

    /// Returns whether the entry matches the filter of the lists pane or only contains a
    /// match.
    #[must_use]
    pub fn role(&self) -> FilterRole {
        self.role
    }
}

impl SearchItem for TreeItem<'_> {
    /// Lists match by name and, with terms, if they contain a task that matches the terms;
    /// folders are only shown because they contain a match, and the bin never while
    /// filtering.
    fn matches(&self, query: &Query) -> bool {
        match self.row {
            TreeRow::Entry(Entry::List(list)) => {
                query.words().matches(list.name())
                    && (!query.has_terms() || term_matches(list, query).next().is_some())
            }
            TreeRow::Entry(Entry::Folder(_)) | TreeRow::Bin(_) => false,
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

/// Returns the tasks of `list` that match the terms of `query`, completed ones included; none
/// if the list could not be loaded.
pub fn term_matches<'list>(
    list: &'list List,
    query: &Query,
) -> impl Iterator<Item = &'list TaskLine> {
    list.content()
        .ok()
        .into_iter()
        .flat_map(|loaded| loaded.document().tasks())
        .map(|node| node.task())
        .filter(|task| query.matches_terms(*task))
}

/// Returns the folders and lists of `workspace` that `hidden_entries` shows, the entries of
/// every folder in `order`, parents before their entries, followed by the row of the bin unless
/// it is empty, ready for an [`Outline`].
#[must_use]
pub fn tree_items(
    workspace: &Workspace,
    hidden_entries: HiddenEntries,
    order: ListOrder,
) -> Vec<TreeItem<'_>> {
    let mut items = Vec::new();
    match order {
        // The workspace keeps its entries in this order, so nothing needs to be summed up.
        ListOrder::Name => push_items(&mut items, workspace.entries(), 0),
        ListOrder::Due | ListOrder::Priority | ListOrder::OpenTasks | ListOrder::TotalTasks => {
            let ordered = ordered_entries(workspace.entries(), order, hidden_entries);
            push_ordered_items(&mut items, &ordered, 0);
        }
    }
    if !workspace.bin_entries().is_empty() {
        items.push(TreeItem {
            row: TreeRow::Bin(workspace.bin_entries()),
            key: EntryKey::Bin(workspace.bin().as_path().to_path_buf()),
            depth: 0,
            role: FilterRole::Match,
        });
    }
    prune(items, |item| match item.row {
        TreeRow::Entry(entry) => hidden_entries.shows(entry),
        TreeRow::Bin(_) => true,
    })
    .collect()
}

fn push_items<'workspace>(
    items: &mut Vec<TreeItem<'workspace>>,
    entries: &'workspace [Entry],
    depth: usize,
) {
    for entry in entries {
        items.push(TreeItem::new(entry, depth));
        match entry {
            Entry::Folder(folder) => push_items(items, folder.entries(), depth + 1),
            Entry::List(_) => {}
        }
    }
}

/// Pushes the items of `entries` and of everything inside them, in their order.
fn push_ordered_items<'workspace>(
    items: &mut Vec<TreeItem<'workspace>>,
    entries: &[OrderedEntry<'workspace>],
    depth: usize,
) {
    for ordered in entries {
        items.push(TreeItem::new(ordered.entry(), depth));
        push_ordered_items(items, ordered.entries(), depth + 1);
    }
}

/// The lists pane: the selection and folding of the workspace tree, its search filter, whether
/// it shows hidden entries, its order, the active list, whose tasks the tasks pane shows, and
/// the bin view while it is open instead of the tree.
#[derive(Debug, Clone)]
pub struct ListsPane {
    outline: Outline<EntryKey>,
    active: Option<ListPath>,
    filter: PaneFilter<EntryKey>,
    hidden_entries: HiddenEntries,
    order: ListOrder,
    bin: Option<BinView>,
}

impl ListsPane {
    /// Selects and activates the first list of `workspace` in `order` that `hidden_entries`
    /// shows, if there is any, in a pane `viewport_height` rows high.
    #[must_use]
    pub fn new(
        workspace: &Workspace,
        hidden_entries: HiddenEntries,
        order: ListOrder,
        viewport_height: usize,
    ) -> Self {
        let items = tree_items(workspace, hidden_entries, order);
        let mut outline = Outline::new();
        let first_list = items.iter().find_map(|item| match &item.key {
            EntryKey::List(path) => Some((&item.key, path)),
            EntryKey::Folder(_) | EntryKey::Bin(_) => None,
        });
        let Some((key, path)) = first_list else {
            outline.refresh(&items, viewport_height);
            return Self {
                outline,
                active: None,
                filter: PaneFilter::default(),
                hidden_entries,
                order,
                bin: None,
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
            order,
            bin: None,
        }
    }

    /// Returns the bin view, or `None` while the pane shows the tree.
    #[must_use]
    pub fn bin_view(&self) -> Option<&BinView> {
        self.bin.as_ref()
    }

    pub(super) fn bin_view_mut(&mut self) -> Option<&mut BinView> {
        self.bin.as_mut()
    }

    /// Shows the bin of `workspace` instead of the tree, from its first entry, in a viewport
    /// `viewport_height` rows high.
    pub(super) fn open_bin(&mut self, workspace: &Workspace, viewport_height: usize) {
        self.bin = Some(BinView::new(workspace, viewport_height));
    }

    /// Shows the tree again instead of the bin.
    pub(super) fn close_bin(&mut self) {
        self.bin = None;
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
            .apply(tree_items(workspace, HiddenEntries::Shown, self.order));
        let shown = self.items(workspace);
        self.outline.select_nearest(&all, &shown, viewport_height);
    }

    /// Returns the order of the folders and lists of every folder.
    #[must_use]
    pub fn order(&self) -> ListOrder {
        self.order
    }

    /// Orders the tree of `workspace` by `order`, keeping the selection in a viewport
    /// `viewport_height` rows high.
    pub(super) fn set_order(
        &mut self,
        workspace: &Workspace,
        order: ListOrder,
        viewport_height: usize,
    ) {
        self.order = order;
        self.refresh(workspace, viewport_height);
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
            .apply(tree_items(workspace, self.hidden_entries, self.order))
    }

    pub(super) fn filter_mut(&mut self) -> &mut PaneFilter<EntryKey> {
        &mut self.filter
    }

    /// Filters the tree of `workspace` again after the filter changed, in a viewport
    /// `viewport_height` rows high.
    pub(super) fn refilter(&mut self, workspace: &Workspace, viewport_height: usize) {
        let (hidden_entries, order) = (self.hidden_entries, self.order);
        self.filter.refilter(
            &mut self.outline,
            || tree_items(workspace, hidden_entries, order),
            viewport_height,
        );
    }

    /// Keeps the selection inside a viewport `viewport_height` rows high, for example after
    /// the terminal was resized.
    pub(super) fn refresh(&mut self, workspace: &Workspace, viewport_height: usize) {
        let items = self.items(workspace);
        self.outline.refresh(&items, viewport_height);
        if let Some(bin) = &mut self.bin {
            bin.refresh(workspace, viewport_height);
        }
    }

    /// Returns the folder that receives a new entry, relative to the workspace root: the
    /// selected folder, the folder of the selected list, or the root (an empty path).
    pub(super) fn target_folder(&self) -> PathBuf {
        match self.outline.selected() {
            Some(EntryKey::Folder(folder)) => folder.clone(),
            Some(EntryKey::List(list)) => list.as_path().parent().map_or_default(Path::to_path_buf),
            Some(EntryKey::Bin(_)) | None => PathBuf::new(),
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
            .and_then(|item| item.entry())
    }

    /// Follows `change` of the workspace, which is already applied to `workspace`: renamed
    /// entries stay selected, folded and active, a removed active list is closed and a removed
    /// selection moves to a neighbour in a viewport `viewport_height` rows high, an entry before
    /// it rather than the row of the bin.
    fn apply_entry_change(
        &mut self,
        workspace: &Workspace,
        change: EntryChange<'_>,
        viewport_height: usize,
    ) {
        let on_bin =
            |outline: &Outline<EntryKey>| matches!(outline.selected(), Some(EntryKey::Bin(_)));
        let was_on_bin = on_bin(&self.outline);
        self.outline.rename_keys(|key| change.renamed_key(key));
        self.active = self.active.take().and_then(|path| change.list_path(path));
        self.refresh(workspace, viewport_height);
        if !was_on_bin && on_bin(&self.outline) {
            let items = self.items(workspace);
            // Without an entry before the bin the selection stays on it, which is fine.
            let _: Outcome = self
                .outline
                .apply(&items, OutlineCommand::Up, viewport_height);
        }
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

    /// Performs `action` on the tree of `workspace`, or on the bin while it is open, shown in
    /// a viewport `viewport_height` rows high, and returns whether it opened a list.
    ///
    /// `open` opens a selected list or the bin and folds or unfolds a selected folder;
    /// `expand` opens a list or the bin when there is nothing to unfold.
    fn update(&mut self, workspace: &Workspace, action: Action, viewport_height: usize) -> bool {
        if let Some(bin) = &mut self.bin {
            bin.update(workspace, action, viewport_height);
            return false;
        }
        let command = if action == Action::Open {
            match self.open_selected(workspace, viewport_height) {
                Opened::List => return true,
                Opened::Bin => return false,
                Opened::Nothing => OutlineCommand::ToggleFold,
            }
        } else if let Some(command) = OutlineCommand::from_action(action) {
            command
        } else {
            return false;
        };
        let items = self.items(workspace);
        let outcome = self.outline.apply(&items, command, viewport_height);
        if command != OutlineCommand::Expand || outcome != Outcome::Unchanged {
            return false;
        }
        match self.open_selected(workspace, viewport_height) {
            Opened::List => true,
            Opened::Bin | Opened::Nothing => false,
        }
    }

    /// Selects the list at `path` in the tree of `workspace` and makes it the active one, in a
    /// viewport `viewport_height` rows high; the tree is shown again if the bin was open.
    pub(super) fn open(&mut self, workspace: &Workspace, path: &ListPath, viewport_height: usize) {
        self.close_bin();
        self.select(workspace, &EntryKey::List(path.clone()), viewport_height);
        self.active = Some(path.clone());
    }

    /// Makes the selected list the active one or opens the bin of `workspace` in a viewport
    /// `viewport_height` rows high, and tells which it did.
    fn open_selected(&mut self, workspace: &Workspace, viewport_height: usize) -> Opened {
        match self.outline.selected() {
            Some(EntryKey::List(path)) => {
                self.active = Some(path.clone());
                Opened::List
            }
            Some(EntryKey::Bin(_)) => {
                self.open_bin(workspace, viewport_height);
                Opened::Bin
            }
            Some(EntryKey::Folder(_)) | None => Opened::Nothing,
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

    /// Asks to confirm moving the selected list or folder to the bin.
    pub(super) fn delete_selected_entry(&mut self) {
        if let Some(entry) = self.lists.selected_entry(&self.workspace) {
            self.ask_to_delete_entries(&[EntryKey::from(entry)]);
        }
    }

    /// Asks to confirm moving the lists and folders `entries` that are still in the workspace
    /// to the bin, as [`Effect::DeleteEntries`](crate::app::Effect::DeleteEntries).
    ///
    /// Every deletion of lists and folders starts here. The question says what moves, folders
    /// with everything inside them.
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

    /// Opens the folder picker for the folder that receives the selected lists and folders in
    /// visual mode, or the selected entry otherwise.
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
            | Mode::FolderPicker(_)
            | Mode::GoTo(_)
            | Mode::Confirm(_)
            | Mode::InlineEdit(_)
            | Mode::Search(_) => None,
        };
        if let Some(entries) = entries {
            self.open_folder_picker(FolderPurpose::Move(entries));
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

/// What opening the selected row of the tree did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Opened {
    /// It made the selected list the active one.
    List,
    /// It opened the bin.
    Bin,
    /// The selected row is a folder, or nothing is selected.
    Nothing,
}

/// Checks that `folder`, relative to the root of `workspace`, can receive the lists and folders
/// `entries`: it must be a folder of the workspace, or the root, and not one of the folders or
/// inside one of them.
///
/// # Errors
///
/// Returns why `folder` cannot receive `entries`, to be shown in the folder picker.
pub(super) fn check_move_target(
    workspace: &Workspace,
    entries: &[EntryKey],
    folder: &Path,
) -> Result<(), String> {
    if !workspace.contains_folder(folder) {
        return Err(format!("there is no folder {}", portable(folder)));
    }
    let moved_into_itself = entries.iter().find_map(|entry| match entry {
        EntryKey::Folder(path) if folder.starts_with(path) => Some(path),
        EntryKey::Folder(_) | EntryKey::List(_) | EntryKey::Bin(_) => None,
    });
    match moved_into_itself {
        Some(path) => Err(format!("cannot move folder {} into itself", portable(path))),
        None => Ok(()),
    }
}

/// Shown when `move-to-folder` is used on a selection of tasks.
const ONLY_ENTRIES_ARE_MOVED: &str = "only lists and folders can be moved into a folder";

/// Asks whether to move `entries` to the bin, saying what moves, or tells that there are no
/// entries at all.
fn delete_question(entries: &[&Entry]) -> Result<String, String> {
    let description = match entries {
        [] => return Err("the selected lists are gone".to_owned()),
        [entry] => entry_description(entry),
        several => several_description(several),
    };
    Ok(format!("Move {description} to the bin?"))
}

/// Names `entry` with what it holds, as a question about it says it: `list "Groceries" with 7
/// tasks`, `folder "Work" with 5 lists and 42 tasks` (lists at any depth) or `folder "Old"`.
pub(crate) fn entry_description(entry: &Entry) -> String {
    match entry {
        Entry::List(list) => match list.content() {
            Ok(_) => format!(
                "list \"{}\" with {}",
                list.name(),
                counted_tasks(task_count(list))
            ),
            Err(_) => format!("list \"{}\"", list.name()),
        },
        Entry::Folder(folder) => folder_description(folder),
    }
}

/// Names `folder` with the number of lists inside it, at any depth, and their tasks.
fn folder_description(folder: &Folder) -> String {
    let (lists, tasks) = count_lists_and_tasks(folder.entries());
    let name = folder.name();
    if lists == 0 {
        return format!("folder \"{name}\"");
    }
    format!(
        "folder \"{name}\" with {} and {}",
        counted(lists, "list", "lists"),
        counted_tasks(tasks)
    )
}

/// Returns the number of lists in `entries`, at any depth, and of their tasks.
pub(super) fn count_lists_and_tasks<'workspace>(
    entries: impl IntoIterator<Item = &'workspace Entry>,
) -> (usize, usize) {
    entries
        .into_iter()
        .fold((0, 0), |(lists, tasks), entry| match entry {
            Entry::List(list) => (lists + 1, tasks + task_count(list)),
            Entry::Folder(folder) => {
                let (inner_lists, inner_tasks) = count_lists_and_tasks(folder.entries());
                (lists + inner_lists, tasks + inner_tasks)
            }
        })
}

/// Names several lists and folders by counting both, such as `12 lists and 2 folders`.
fn several_description(entries: &[&Entry]) -> String {
    let lists = entries
        .iter()
        .filter(|entry| matches!(entry, Entry::List(_)))
        .count();
    let folders = entries.len() - lists;
    let lists = (lists > 0).then(|| counted(lists, "list", "lists"));
    let folders = (folders > 0).then(|| counted(folders, "folder", "folders"));
    let parts: Vec<String> = lists.into_iter().chain(folders).collect();
    parts.join(" and ")
}

/// Returns `count` with the `singular` noun for one and the `plural` otherwise.
pub(super) fn counted(count: usize, singular: &str, plural: &str) -> String {
    let noun = if count == 1 { singular } else { plural };
    format!("{count} {noun}")
}

/// Returns the number of tasks of `list`, `0` if it could not be loaded.
fn task_count(list: &List) -> usize {
    list.content()
        .map_or(0, |loaded| loaded.document().progress().total())
}

#[cfg(test)]
mod tests {
    use std::fs;

    use rstest::rstest;
    use tempfile::TempDir;

    use super::*;
    use crate::config::IndentStyle;
    use crate::storage::bin::BinPath;

    /// A workspace with the empty folders `empty` and `other`, the folder `home` with
    /// `chores` (two tasks), the folder `outer` with two lists with three tasks in itself and
    /// its folder `inner`, and the lists `inbox` (one task) and `binary`, which cannot be
    /// loaded.
    fn workspace(root: &TempDir) -> Workspace {
        for folder in ["empty", "other", "home"] {
            fs::create_dir(root.path().join(folder)).unwrap();
        }
        fs::write(root.path().join("home/chores.md"), "- [ ] a\n- [ ] b\n").unwrap();
        fs::write(root.path().join("inbox.md"), "- [ ] c\n").unwrap();
        fs::write(root.path().join("binary.md"), [0xff]).unwrap();
        fs::create_dir_all(root.path().join("outer/inner")).unwrap();
        fs::write(root.path().join("outer/a.md"), "- [ ] a\n").unwrap();
        fs::write(root.path().join("outer/inner/b.md"), "- [ ] b\n- [x] c\n").unwrap();
        Workspace::scan(root.path(), BinPath::default(), IndentStyle::default()).unwrap()
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
    #[case::one_list("inbox.md", Ok("Move list \"inbox\" with 1 task to the bin?"))]
    #[case::unreadable_list("binary.md", Ok("Move list \"binary\" to the bin?"))]
    #[case::empty_folder("empty", Ok("Move folder \"empty\" to the bin?"))]
    #[case::folder("home", Ok("Move folder \"home\" with 1 list and 2 tasks to the bin?"))]
    #[case::nested_folders(
        "outer",
        Ok("Move folder \"outer\" with 2 lists and 3 tasks to the bin?")
    )]
    #[case::lists("inbox.md home/chores.md", Ok("Move 2 lists to the bin?"))]
    #[case::folders("empty other", Ok("Move 2 folders to the bin?"))]
    #[case::both("empty inbox.md home", Ok("Move 1 list and 2 folders to the bin?"))]
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
        let workspace =
            Workspace::scan(root.path(), BinPath::default(), IndentStyle::default()).unwrap();
        let expected = [("home", 0), ("empty", 1), ("chores", 1), ("inbox", 0)];

        let system_under_test: Vec<(&str, usize)> =
            tree_items(&workspace, HiddenEntries::Hidden, ListOrder::Name)
                .iter()
                .map(|item| (item.entry().unwrap().name(), item.depth()))
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
        let workspace =
            Workspace::scan(root.path(), BinPath::default(), IndentStyle::default()).unwrap();

        let system_under_test: Vec<(&str, usize)> =
            tree_items(&workspace, hidden_entries, ListOrder::Name)
                .iter()
                .map(|item| (item.entry().unwrap().name(), item.depth()))
                .collect();

        assert_eq!(system_under_test, expected);
    }

    #[rstest]
    #[case::shown(HiddenEntries::Shown)]
    #[case::hidden(HiddenEntries::Hidden)]
    fn tree_items_end_with_the_bin_unless_it_is_empty(#[case] hidden_entries: HiddenEntries) {
        let root = TempDir::new().unwrap();
        fs::create_dir_all(root.path().join(".trash")).unwrap();
        fs::write(root.path().join(".trash/old.md"), "").unwrap();
        fs::write(root.path().join("inbox.md"), "").unwrap();
        let workspace =
            Workspace::scan(root.path(), BinPath::default(), IndentStyle::default()).unwrap();
        let expected = EntryKey::Bin(PathBuf::from(".trash"));

        let system_under_test = tree_items(&workspace, hidden_entries, ListOrder::Name);

        assert_eq!(system_under_test.len(), 2);
        assert_eq!(system_under_test[1].key(), &expected);
        assert!(matches!(system_under_test[1].row(), TreeRow::Bin([_])));
        assert!(!system_under_test[1].matches(&Query::default()));
    }

    #[rstest]
    #[case::list("inbox.md", "list \"inbox\" with 1 task")]
    #[case::unreadable_list("binary.md", "list \"binary\"")]
    #[case::empty_folder("empty", "folder \"empty\"")]
    #[case::folder("outer", "folder \"outer\" with 2 lists and 3 tasks")]
    fn entry_description_names_the_entry_with_what_it_holds(
        #[case] path: &str,
        #[case] expected: &str,
    ) {
        let root = TempDir::new().unwrap();
        let workspace = workspace(&root);
        let entry = workspace.entry(Path::new(path)).unwrap();

        let system_under_test = entry_description(entry);

        assert_eq!(system_under_test, expected);
    }
}

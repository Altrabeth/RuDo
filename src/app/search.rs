//! Filtering the lists and tasks pane with `search` (`/`): the [`PaneFilter`] of a pane and
//! the search prompt of [`Mode::Search`] in the bottom bar, which filters the pane live.
//!
//! A filter keeps the items that match its [`SmartCaseQuery`] together with their ancestors,
//! through [`keep_matches`]; the tasks pane applies it after hiding completed tasks with
//! [`prune`](crate::ui::outline::prune), so a match below a hidden task stays hidden. Folds are
//! suspended while a filter is active. Items that the application selects for the user, such
//! as a task just added with `o`, are revealed: they stay visible until the query changes.

use std::collections::HashSet;
use std::fmt;
use std::hash::Hash;

use crate::app::filter::SmartCaseQuery;
use crate::app::state::{App, Focus, Mode};
use crate::config::keymap::key::KeyChord;
use crate::ui::outline::{FilterRole, Folding, Outline, OutlineItem, keep_matches};
use crate::ui::text_input::{TextChange, TextInput};

/// Written before the query, in the search prompt and in the pane titles, as in vim.
pub const PROMPT: &str = "/";

/// An item of a pane that a [`PaneFilter`] can keep.
pub(super) trait SearchItem: OutlineItem + Sized {
    /// Returns the text the query is matched against, or `None` if the item is only ever
    /// shown as the ancestor of a match, as a folder is.
    fn search_text(&self) -> Option<&str>;

    /// Returns the item, telling why it is shown.
    #[must_use]
    fn with_role(self, role: FilterRole) -> Self;
}

/// The search filter of a pane: the typed text, the query it means, and the items revealed
/// while it is active.
///
/// An empty filter is inactive and shows every item.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaneFilter<K: Eq + Hash> {
    input: TextInput,
    query: SmartCaseQuery,
    revealed: HashSet<K>,
}

impl<K: Eq + Hash> Default for PaneFilter<K> {
    fn default() -> Self {
        Self {
            input: TextInput::new(),
            query: SmartCaseQuery::default(),
            revealed: HashSet::new(),
        }
    }
}

impl<K: Eq + Hash> PaneFilter<K> {
    /// Returns the input with the typed text, as the search prompt draws it.
    #[must_use]
    pub fn input(&self) -> &TextInput {
        &self.input
    }

    /// Returns the query whose matches the pane highlights, or `None` while the filter is
    /// inactive.
    #[must_use]
    pub fn query(&self) -> Option<&SmartCaseQuery> {
        (!self.query.is_empty()).then_some(&self.query)
    }

    /// Tells whether the filter hides items, that is whether any text was typed.
    #[must_use]
    pub fn is_active(&self) -> bool {
        !self.query.is_empty()
    }

    /// Returns whether the folds of the pane apply: they are suspended while the filter is
    /// active, so that no match is folded away.
    pub(super) fn folding(&self) -> Folding {
        if self.is_active() {
            Folding::Suspended
        } else {
            Folding::Applied
        }
    }

    /// Returns the matches of `items` with their ancestors, or every item while the filter is
    /// inactive.
    pub(super) fn apply<I: SearchItem<Key = K>>(
        &self,
        items: impl IntoIterator<Item = I>,
    ) -> Vec<I> {
        if !self.is_active() {
            return items.into_iter().collect();
        }
        keep_matches(items, |item| {
            self.revealed.contains(item.key())
                || item
                    .search_text()
                    .is_some_and(|text| self.query.matches(text))
        })
        .map(|(item, role)| item.with_role(role))
        .collect()
    }

    /// Keeps the item with `key` visible while the filter is active, whether it matches or
    /// not, until the query changes.
    pub(super) fn reveal(&mut self, key: K) {
        if self.is_active() {
            self.revealed.insert(key);
        }
    }

    /// Forgets the revealed items, whose keys are void, for example after a reload.
    pub(super) fn forget_revealed(&mut self) {
        self.revealed.clear();
    }

    /// Brings `outline` in line with the filter after the filter changed: the folds are
    /// suspended while it is active, and a selection that `items` no longer shows moves to
    /// the nearest one that is.
    ///
    /// `items` returns the items before filtering, every time it is called.
    pub(super) fn refilter<I: SearchItem<Key = K>, Items: IntoIterator<Item = I>>(
        &self,
        outline: &mut Outline<K>,
        items: impl Fn() -> Items,
        viewport_height: usize,
    ) where
        K: Clone,
    {
        outline.set_folding(self.folding());
        let all: Vec<I> = items().into_iter().collect();
        let shown = self.apply(items());
        outline.select_nearest(&all, &shown, viewport_height);
    }

    /// Edits the typed text with `edit`; a changed text is a new query, which reveals nothing.
    fn edit(&mut self, edit: impl FnOnce(&mut TextInput) -> TextChange) -> TextChange {
        let change = edit(&mut self.input);
        match change {
            TextChange::Changed => {
                self.query = SmartCaseQuery::new(self.input.value());
                self.revealed.clear();
            }
            TextChange::Unchanged => {}
        }
        change
    }
}

impl<K: Eq + Hash> fmt::Display for PaneFilter<K> {
    /// Writes the filter as typed after the [`PROMPT`], such as `/milk`.
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{PROMPT}{}", self.input.value())
    }
}

/// Empties `input`.
fn clear(input: &mut TextInput) -> TextChange {
    if input.value().is_empty() {
        return TextChange::Unchanged;
    }
    *input = TextInput::new();
    TextChange::Changed
}

impl App {
    /// Returns the text typed into the search prompt while [`Mode::Search`] is open.
    #[must_use]
    pub fn search_input(&self) -> Option<&TextInput> {
        match self.mode {
            Mode::Search(pane) => self.filter_input(pane),
            Mode::Normal
            | Mode::Help(_)
            | Mode::Prompt(_)
            | Mode::Confirm(_)
            | Mode::InlineEdit(_)
            | Mode::Visual => None,
        }
    }

    /// Opens the search prompt for the focused pane, continuing its filter, and discards a
    /// pending key sequence. The tasks pane has nothing to search while no list is open.
    pub(super) fn open_search(&mut self) {
        if self.filter_input(self.focus).is_none() {
            return;
        }
        self.resolver.reset();
        self.mode = Mode::Search(self.focus);
    }

    /// Closes the search prompt and keeps the filter.
    pub(super) fn submit_search(&mut self) {
        self.close_search();
    }

    /// Closes the search prompt and removes the filter of `pane`.
    pub(super) fn cancel_search(&mut self, pane: Focus) {
        self.close_search();
        self.edit_filter(pane, clear);
    }

    /// Removes the filter of the focused pane.
    pub(super) fn clear_filter(&mut self) {
        self.edit_filter(self.focus, clear);
    }

    /// Edits the search query of `pane` with a key press that no action is bound to.
    pub(super) fn type_search(&mut self, pane: Focus, chord: KeyChord) {
        self.edit_filter(pane, |input| input.press(chord));
    }

    /// Inserts pasted text into the search query of `pane`.
    pub(super) fn paste_search(&mut self, pane: Focus, text: &str) {
        self.edit_filter(pane, |input| {
            input.insert_str(text);
            TextChange::Changed
        });
    }

    /// Leaves the search prompt and discards a pending key sequence.
    fn close_search(&mut self) {
        self.resolver.reset();
        self.mode = Mode::Normal;
    }

    /// Returns the typed text of the filter of `pane`, or `None` for the tasks pane while no
    /// list is open.
    fn filter_input(&self, pane: Focus) -> Option<&TextInput> {
        match pane {
            Focus::Lists => Some(self.lists.filter().input()),
            Focus::Tasks => self.task_filter().map(PaneFilter::input),
        }
    }

    /// Edits the filter of `pane` with `edit` and filters the pane again if the text changed.
    fn edit_filter(&mut self, pane: Focus, edit: impl FnOnce(&mut TextInput) -> TextChange) {
        let change = match pane {
            Focus::Lists => self.lists.filter_mut().edit(edit),
            Focus::Tasks => match self.lists.active() {
                Some(path) => self.tasks.filter_mut(path).edit(edit),
                None => TextChange::Unchanged,
            },
        };
        match (change, pane) {
            (TextChange::Changed, Focus::Lists) => {
                let viewport_height = self.viewport_height(Focus::Lists);
                self.lists.refilter(&self.workspace, viewport_height);
            }
            (TextChange::Changed, Focus::Tasks) => self.refilter_tasks(),
            (TextChange::Unchanged, Focus::Lists | Focus::Tasks) => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;
    use tempfile::TempDir;

    use super::*;
    use crate::app::state::tests::app_in;
    use crate::config::keymap::action::Action;

    /// An item with a key, a depth, an optional search text and a role.
    #[derive(Debug, PartialEq, Eq)]
    struct Item {
        key: char,
        depth: usize,
        text: Option<&'static str>,
        role: FilterRole,
    }

    impl OutlineItem for Item {
        type Key = char;

        fn key(&self) -> &char {
            &self.key
        }

        fn depth(&self) -> usize {
            self.depth
        }
    }

    impl SearchItem for Item {
        fn search_text(&self) -> Option<&str> {
            self.text
        }

        fn with_role(self, role: FilterRole) -> Self {
            Self { role, ..self }
        }
    }

    /// The folder `f` with the lists `milk` and `bread`, and the list `work`.
    fn items() -> Vec<Item> {
        [
            ('f', 0, None),
            ('m', 1, Some("milk")),
            ('b', 1, Some("bread")),
            ('w', 0, Some("work")),
        ]
        .into_iter()
        .map(|(key, depth, text)| Item {
            key,
            depth,
            text,
            role: FilterRole::Match,
        })
        .collect()
    }

    fn filter(text: &str) -> PaneFilter<char> {
        let mut filter = PaneFilter::default();
        let _: TextChange = filter.edit(|input| {
            input.insert_str(text);
            TextChange::Changed
        });
        filter
    }

    fn shown(filter: &PaneFilter<char>) -> Vec<(char, FilterRole)> {
        filter
            .apply(items())
            .into_iter()
            .map(|item| (item.key, item.role))
            .collect()
    }

    #[test]
    fn inactive_filter_shows_every_item() {
        let filter = filter("");
        let expected = [
            ('f', FilterRole::Match),
            ('m', FilterRole::Match),
            ('b', FilterRole::Match),
            ('w', FilterRole::Match),
        ];

        let system_under_test = shown(&filter);

        assert_eq!(system_under_test, expected);
        assert!(!filter.is_active());
        assert_eq!(filter.query(), None);
    }

    #[rstest]
    #[case::match_with_its_folder("milk", &[('f', FilterRole::Context), ('m', FilterRole::Match)])]
    #[case::smart_case("MILK", &[])]
    #[case::folder_text_never_matches("f", &[])]
    #[case::top_level("or", &[('w', FilterRole::Match)])]
    fn active_filter_shows_matches_with_their_ancestors(
        #[case] text: &str,
        #[case] expected: &[(char, FilterRole)],
    ) {
        let filter = filter(text);

        let system_under_test = shown(&filter);

        assert_eq!(system_under_test, expected);
    }

    #[test]
    fn revealed_item_is_shown_although_it_does_not_match() {
        let mut filter = filter("milk");
        filter.reveal('w');
        let expected = [
            ('f', FilterRole::Context),
            ('m', FilterRole::Match),
            ('w', FilterRole::Match),
        ];

        let system_under_test = shown(&filter);

        assert_eq!(system_under_test, expected);
    }

    #[test]
    fn changing_the_query_forgets_revealed_items() {
        let mut filter = filter("mil");
        filter.reveal('w');
        let _: TextChange = filter.edit(|input| input.press("k".parse().unwrap()));
        let expected = [('f', FilterRole::Context), ('m', FilterRole::Match)];

        let system_under_test = shown(&filter);

        assert_eq!(system_under_test, expected);
    }

    #[test]
    fn inactive_filter_reveals_nothing() {
        let mut filter = filter("");
        filter.reveal('w');

        let system_under_test = filter.revealed;

        assert!(system_under_test.is_empty());
    }

    #[test]
    fn forgetting_revealed_items_hides_them_again() {
        let mut filter = filter("milk");
        filter.reveal('w');
        filter.forget_revealed();
        let expected = [('f', FilterRole::Context), ('m', FilterRole::Match)];

        let system_under_test = shown(&filter);

        assert_eq!(system_under_test, expected);
    }

    #[rstest]
    #[case::empty("", "/")]
    #[case::query("milk", "/milk")]
    fn filter_is_displayed_after_the_prompt(#[case] text: &str, #[case] expected: &str) {
        let filter = filter(text);

        let system_under_test = filter.to_string();

        assert_eq!(system_under_test, expected);
    }

    #[test]
    fn clearing_an_empty_input_changes_nothing() {
        let mut input = TextInput::new();

        let system_under_test = clear(&mut input);

        assert_eq!(system_under_test, TextChange::Unchanged);
    }

    #[test]
    fn search_in_the_tasks_pane_without_a_list_stays_in_normal_mode() {
        let directory = TempDir::new().unwrap();
        let mut app = app_in(directory.path());
        app.focus = Focus::Tasks;
        let _ = app.update(Action::Search);

        let system_under_test = app.mode();

        assert!(matches!(system_under_test, Mode::Normal));
        assert_eq!(app.search_input(), None);
    }

    #[test]
    fn typing_into_the_tasks_filter_without_a_list_changes_nothing() {
        let directory = TempDir::new().unwrap();
        let mut app = app_in(directory.path());
        app.type_search(Focus::Tasks, "x".parse().unwrap());

        let system_under_test = app.task_filter();

        assert_eq!(system_under_test, None);
    }
}

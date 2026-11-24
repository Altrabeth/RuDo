//! What the help screen shows, generated from the effective [`Keymap`].
//!
//! [`help_sections`] lists every [`Action`] once, with the keys the user actually configured,
//! in the sections of the README keybinding tables, followed by the fixed [`EDITING_KEYS`] of
//! text inputs. [`filter`] narrows the sections to a search query and [`rows`] flattens them
//! for scrolling. The model is pure, so its correctness is tested without rendering.

use std::fmt::{self, Display, Formatter, Write};

use strum::IntoEnumIterator;

use crate::app::filter::SmartCaseQuery;
use crate::config::keymap::action::Action;
use crate::config::keymap::key::KeySequence;
use crate::config::keymap::keymap::Keymap;
use crate::ui::outline::OutlineItem;
use crate::ui::text_input::{EDITING_KEYS, EditCommand, EditingKey};

/// Shown instead of the keys of an action without keys, so that it can still be discovered.
const UNBOUND: &str = "unbound";

/// Separates the key sequences of one entry.
const KEY_SEPARATOR: &str = ", ";

/// A section of the help screen, mirroring the keybinding tables of the README.
///
/// Displays as the section title, such as `Tasks pane`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, strum::EnumIter)]
#[non_exhaustive]
pub enum HelpHeading {
    /// Actions of the lists and tasks pane that do not depend on the selection.
    Everywhere,
    /// Moving the selection, also on the help screen.
    MovingAround,
    /// Folding and tree navigation in the lists and tasks pane.
    Trees,
    /// Actions on lists and folders.
    ListsPane,
    /// Actions on tasks.
    TasksPane,
    /// Closing the help screen, answering dialogs and finishing text input.
    HelpAndDialogs,
    /// The fixed editing keys of text inputs.
    EditingKeys,
}

impl Display for HelpHeading {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Everywhere => "Everywhere (lists and tasks pane)",
            Self::MovingAround => "Moving around (lists pane, tasks pane and help screen)",
            Self::Trees => "Trees (lists pane and tasks pane)",
            Self::ListsPane => "Lists pane",
            Self::TasksPane => "Tasks pane",
            Self::HelpAndDialogs => "Help screen, dialogs and text input",
            Self::EditingKeys => "Editing keys in text input",
        })
    }
}

/// What a help entry explains: a configurable action or a fixed editing command.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HelpSubject {
    /// An action bound through the keymap.
    Action(Action),
    /// An editing command of text inputs with fixed keys.
    Edit(EditCommand),
}

impl HelpSubject {
    /// Returns the name used in the configuration file, if the subject can be configured.
    #[must_use]
    pub fn name(self) -> Option<&'static str> {
        match self {
            Self::Action(action) => Some(action.into()),
            Self::Edit(_) => None,
        }
    }
}

/// One line of the help screen: the keys, the description and what they trigger.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct HelpEntry {
    subject: HelpSubject,
    /// The keys in compact form, separated by commas; `None` for an unbound action.
    keys: Option<String>,
    description: &'static str,
}

impl HelpEntry {
    fn for_action(action: Action, keys: &[KeySequence]) -> Self {
        Self {
            subject: HelpSubject::Action(action),
            keys: (!keys.is_empty()).then(|| compact_keys(keys)),
            description: action.description(),
        }
    }

    fn for_editing_key(editing_key: &EditingKey) -> Self {
        Self {
            subject: HelpSubject::Edit(editing_key.command()),
            keys: Some(editing_key.keys().join(KEY_SEPARATOR)),
            description: editing_key.description(),
        }
    }

    /// Returns what the entry explains.
    #[must_use]
    pub const fn subject(&self) -> HelpSubject {
        self.subject
    }

    /// Returns the keys in compact form, such as `j, down` or `zM`, or `unbound` for an
    /// action without keys.
    #[must_use]
    pub fn keys(&self) -> &str {
        self.keys.as_deref().unwrap_or(UNBOUND)
    }

    /// Tells whether the entry is an action without keys.
    #[must_use]
    pub const fn is_unbound(&self) -> bool {
        self.keys.is_none()
    }

    /// Returns what the keys do.
    #[must_use]
    pub const fn description(&self) -> &'static str {
        self.description
    }

    fn matches(&self, query: &SmartCaseQuery) -> bool {
        query.matches(self.keys())
            || query.matches(self.description)
            || self.subject.name().is_some_and(|name| query.matches(name))
    }
}

/// A heading with the entries listed below it.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct HelpSection {
    heading: HelpHeading,
    entries: Vec<HelpEntry>,
}

impl HelpSection {
    /// Returns the heading of the section.
    #[must_use]
    pub const fn heading(&self) -> HelpHeading {
        self.heading
    }

    /// Returns the entries of the section in display order.
    #[must_use]
    pub fn entries(&self) -> &[HelpEntry] {
        &self.entries
    }
}

/// A row of the flattened help screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HelpRow<'a> {
    /// The heading of a section.
    Heading(HelpHeading),
    /// An entry below a heading.
    Entry(&'a HelpEntry),
}

impl HelpRow<'_> {
    /// Returns the key that identifies the row while the help screen is filtered and
    /// scrolled.
    #[must_use]
    pub const fn key(self) -> HelpRowKey {
        match self {
            Self::Heading(heading) => HelpRowKey::Heading(heading),
            Self::Entry(entry) => HelpRowKey::Entry(entry.subject),
        }
    }
}

/// Identifies a [`HelpRow`] independently of the sections it was taken from, so that the
/// help screen can select and scroll to it with an [`Outline`](crate::ui::outline::Outline).
///
/// The help screen is flat: every row is a top-level outline item.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HelpRowKey {
    /// The row of a section heading.
    Heading(HelpHeading),
    /// The row of an entry.
    Entry(HelpSubject),
}

impl OutlineItem for HelpRowKey {
    type Key = Self;

    fn key(&self) -> &Self {
        self
    }

    fn depth(&self) -> usize {
        0
    }
}

/// Returns the help screen for `keymap`: every action exactly once, in the section of its
/// most general context, followed by the editing keys of text inputs.
///
/// Actions keep the order of the keymap registry, which is the order of the README tables.
///
/// # Examples
///
/// ```
/// use rudo::config::keymap::keymap::{KeyBindings, Keymap};
/// use rudo::ui::help::{HelpHeading, help_sections};
///
/// let keymap = Keymap::new(&KeyBindings::default()).expect("the default keymap is valid");
/// let sections = help_sections(&keymap);
/// assert_eq!(sections[0].heading(), HelpHeading::Everywhere);
/// assert_eq!(sections[0].entries()[0].keys(), "?");
/// ```
#[must_use]
pub fn help_sections(keymap: &Keymap) -> Vec<HelpSection> {
    HelpHeading::iter()
        .map(|heading| HelpSection {
            heading,
            entries: section_entries(heading, keymap),
        })
        .collect()
}

/// Returns the sections reduced to the entries whose keys, description or action name
/// contain `query` with vim's smart case; sections without matching entries are dropped.
#[must_use]
pub fn filter(sections: &[HelpSection], query: &str) -> Vec<HelpSection> {
    let query = SmartCaseQuery::new(query);
    sections
        .iter()
        .filter_map(|section| {
            let entries: Vec<HelpEntry> = section
                .entries
                .iter()
                .filter(|entry| entry.matches(&query))
                .cloned()
                .collect();
            (!entries.is_empty()).then_some(HelpSection {
                heading: section.heading,
                entries,
            })
        })
        .collect()
}

/// Flattens the sections into the rows of the help screen: each heading followed by its
/// entries.
pub fn rows(sections: &[HelpSection]) -> impl Iterator<Item = HelpRow<'_>> {
    sections.iter().flat_map(|section| {
        std::iter::once(HelpRow::Heading(section.heading))
            .chain(section.entries.iter().map(HelpRow::Entry))
    })
}

fn section_entries(heading: HelpHeading, keymap: &Keymap) -> Vec<HelpEntry> {
    match heading {
        HelpHeading::EditingKeys => EDITING_KEYS
            .iter()
            .map(HelpEntry::for_editing_key)
            .collect(),
        HelpHeading::Everywhere
        | HelpHeading::MovingAround
        | HelpHeading::Trees
        | HelpHeading::ListsPane
        | HelpHeading::TasksPane
        | HelpHeading::HelpAndDialogs => Action::iter()
            .filter(|action| action_heading(*action) == heading)
            .map(|action| HelpEntry::for_action(action, keymap.keys(action)))
            .collect(),
    }
}

/// Returns the section of an action: the one of its most general context, so that an action
/// valid in several contexts (such as `search`) is listed once.
const fn action_heading(action: Action) -> HelpHeading {
    match action {
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
        | Action::OpenInEditor => HelpHeading::Everywhere,
        Action::MoveDown
        | Action::MoveUp
        | Action::MoveToTop
        | Action::MoveToBottom
        | Action::HalfPageDown
        | Action::HalfPageUp => HelpHeading::MovingAround,
        Action::Collapse
        | Action::Expand
        | Action::ToggleFold
        | Action::FoldAll
        | Action::UnfoldAll
        | Action::Delete => HelpHeading::Trees,
        Action::Open | Action::NewList | Action::NewFolder | Action::Rename => {
            HelpHeading::ListsPane
        }
        Action::NewTaskBelow
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
        | Action::SortByStatus => HelpHeading::TasksPane,
        Action::CloseHelp
        | Action::ConfirmYes
        | Action::ConfirmNo
        | Action::Submit
        | Action::Cancel => HelpHeading::HelpAndDialogs,
    }
}

/// Joins the compact forms of `keys` with commas, such as `j, down`.
fn compact_keys(keys: &[KeySequence]) -> String {
    let mut text = String::new();
    for (index, sequence) in keys.iter().enumerate() {
        if index > 0 {
            text.push_str(KEY_SEPARATOR);
        }
        write!(text, "{}", sequence.compact()).expect("writing to a String never fails");
    }
    text
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;
    use crate::config::keymap::keymap::KeyBindings;

    fn keymap(yaml: &str) -> Keymap {
        let overrides: KeyBindings = if yaml.is_empty() {
            KeyBindings::default()
        } else {
            serde_saphyr::from_str(yaml).unwrap()
        };
        Keymap::new(&overrides).unwrap()
    }

    fn entries(sections: &[HelpSection]) -> impl Iterator<Item = &HelpEntry> {
        sections.iter().flat_map(HelpSection::entries)
    }

    fn action_entry(sections: &[HelpSection], action: Action) -> &HelpEntry {
        entries(sections)
            .find(|entry| entry.subject() == HelpSubject::Action(action))
            .unwrap()
    }

    /// Describes a row by the heading title or the entry keys, for readable comparisons.
    fn row_label(row: HelpRow<'_>) -> String {
        match row {
            HelpRow::Heading(heading) => heading.to_string(),
            HelpRow::Entry(entry) => entry.keys().to_owned(),
        }
    }

    #[test]
    fn every_action_appears_exactly_once() {
        let keymap = keymap("");

        let system_under_test = help_sections(&keymap);

        for action in Action::iter() {
            let occurrences = entries(&system_under_test)
                .filter(|entry| entry.subject() == HelpSubject::Action(action))
                .count();
            assert_eq!(occurrences, 1, "{action}");
        }
    }

    #[test]
    fn sections_follow_the_heading_order() {
        let keymap = keymap("");
        let expected: Vec<HelpHeading> = HelpHeading::iter().collect();

        let system_under_test = help_sections(&keymap);

        let headings: Vec<HelpHeading> =
            system_under_test.iter().map(HelpSection::heading).collect();
        assert_eq!(headings, expected);
    }

    #[test]
    fn every_heading_has_entries() {
        let keymap = keymap("");

        let system_under_test = help_sections(&keymap);

        for section in &system_under_test {
            assert!(!section.entries().is_empty(), "{}", section.heading());
        }
    }

    #[test]
    fn actions_keep_the_registry_order_within_a_section() {
        let keymap = keymap("");

        let system_under_test = help_sections(&keymap);

        for section in &system_under_test {
            let actions: Vec<Action> = section
                .entries()
                .iter()
                .filter_map(|entry| match entry.subject() {
                    HelpSubject::Action(action) => Some(action),
                    HelpSubject::Edit(_) => None,
                })
                .collect();
            assert!(actions.is_sorted(), "{}", section.heading());
        }
    }

    #[rstest]
    #[case::global(Action::Quit, HelpHeading::Everywhere)]
    #[case::search_in_its_most_general_section(Action::Search, HelpHeading::Everywhere)]
    #[case::navigation(Action::MoveDown, HelpHeading::MovingAround)]
    #[case::tree(Action::FoldAll, HelpHeading::Trees)]
    #[case::delete_in_both_panes(Action::Delete, HelpHeading::Trees)]
    #[case::lists(Action::NewList, HelpHeading::ListsPane)]
    #[case::tasks(Action::Yank, HelpHeading::TasksPane)]
    #[case::help(Action::CloseHelp, HelpHeading::HelpAndDialogs)]
    #[case::dialog(Action::ConfirmNo, HelpHeading::HelpAndDialogs)]
    #[case::input(Action::Submit, HelpHeading::HelpAndDialogs)]
    fn action_is_listed_in_its_section(#[case] action: Action, #[case] expected: HelpHeading) {
        let keymap = keymap("");

        let system_under_test = help_sections(&keymap);

        let section = system_under_test
            .iter()
            .find(|section| {
                section
                    .entries()
                    .iter()
                    .any(|entry| entry.subject() == HelpSubject::Action(action))
            })
            .unwrap();
        assert_eq!(section.heading(), expected);
    }

    #[rstest]
    #[case::two_keys(Action::MoveDown, "j, down")]
    #[case::sequence_of_characters(Action::FoldAll, "zM")]
    #[case::repeated_character(Action::Delete, "dd")]
    #[case::sequence_then_named_key(Action::MoveToTop, "gg, home")]
    #[case::modifier(Action::HalfPageDown, "ctrl-d, pagedown")]
    #[case::space(Action::ToggleDone, "x, space")]
    fn default_keys_are_shown_compact(#[case] action: Action, #[case] expected: &str) {
        let keymap = keymap("");

        let system_under_test = help_sections(&keymap);

        let entry = action_entry(&system_under_test, action);
        assert_eq!(entry.keys(), expected);
        assert!(!entry.is_unbound());
    }

    #[rstest]
    #[case::list("toggle-done: [t, ctrl-x]", Action::ToggleDone, "t, ctrl-x")]
    #[case::sequence("set-due-date: t d", Action::SetDueDate, "td")]
    fn overridden_keys_appear(#[case] yaml: &str, #[case] action: Action, #[case] expected: &str) {
        let keymap = keymap(yaml);

        let system_under_test = help_sections(&keymap);

        assert_eq!(action_entry(&system_under_test, action).keys(), expected);
    }

    #[test]
    fn unbound_action_is_marked() {
        let keymap = keymap("open-in-editor: []");

        let system_under_test = help_sections(&keymap);

        let entry = action_entry(&system_under_test, Action::OpenInEditor);
        assert!(entry.is_unbound());
        assert_eq!(entry.keys(), "unbound");
        assert_eq!(entry.description(), Action::OpenInEditor.description());
    }

    #[test]
    fn editing_keys_come_from_the_editing_key_table() {
        let keymap = keymap("");
        let expected: Vec<(HelpSubject, String, &str)> = EDITING_KEYS
            .iter()
            .map(|editing_key| {
                (
                    HelpSubject::Edit(editing_key.command()),
                    editing_key.keys().join(", "),
                    editing_key.description(),
                )
            })
            .collect();

        let system_under_test = help_sections(&keymap);

        let section = system_under_test.last().unwrap();
        let listed: Vec<(HelpSubject, String, &str)> = section
            .entries()
            .iter()
            .map(|entry| {
                (
                    entry.subject(),
                    entry.keys().to_owned(),
                    entry.description(),
                )
            })
            .collect();
        assert_eq!(section.heading(), HelpHeading::EditingKeys);
        assert_eq!(listed, expected);
    }

    #[rstest]
    #[case::action(Action::SortByDueDate, Some("sort-by-due-date"))]
    #[case::single_word(Action::Quit, Some("quit"))]
    fn action_subject_is_named_like_the_configuration(
        #[case] action: Action,
        #[case] expected: Option<&str>,
    ) {
        let system_under_test = HelpSubject::Action(action);

        assert_eq!(system_under_test.name(), expected);
    }

    #[test]
    fn edit_subject_has_no_name() {
        let system_under_test = HelpSubject::Edit(EditCommand::DeleteToEnd);

        assert_eq!(system_under_test.name(), None);
    }

    #[rstest]
    #[case::by_compact_key("", "zM", &["Trees (lists pane and tasks pane)", "zM"])]
    #[case::by_action_name(
        "",
        "sort-by-due",
        &["Tasks pane", "sd"],
    )]
    #[case::uppercase_description_respects_case("", "CLOSE THE HELP", &[])]
    #[case::by_description(
        "",
        "close the help",
        &["Help screen, dialogs and text input", "?, q, esc"],
    )]
    #[case::uppercase_respects_case("", "Undo", &[])]
    #[case::lowercase_ignores_case("", "quit rudo", &["Everywhere (lists and tasks pane)", "q, ctrl-c"])]
    #[case::editing_key(
        "",
        "ctrl-w",
        &["Editing keys in text input", "ctrl-w"],
    )]
    #[case::unbound_marker(
        "open-in-editor: []",
        "unbound",
        &["Everywhere (lists and tasks pane)", "unbound"],
    )]
    #[case::overridden_key(
        "toggle-done: ctrl-x",
        "ctrl-x",
        &["Tasks pane", "ctrl-x"],
    )]
    #[case::several_sections(
        "",
        "pagedown",
        &["Moving around (lists pane, tasks pane and help screen)", "ctrl-d, pagedown"],
    )]
    #[case::no_match("", "no such thing", &[])]
    fn filter_keeps_matching_entries_and_drops_empty_sections(
        #[case] yaml: &str,
        #[case] query: &str,
        #[case] expected: &[&str],
    ) {
        let sections = help_sections(&keymap(yaml));

        let system_under_test = filter(&sections, query);

        let labels: Vec<String> = rows(&system_under_test).map(row_label).collect();
        assert_eq!(labels, expected);
    }

    #[test]
    fn empty_filter_keeps_everything() {
        let sections = help_sections(&keymap(""));

        let system_under_test = filter(&sections, "");

        assert_eq!(system_under_test, sections);
    }

    #[test]
    fn rows_list_each_heading_before_its_entries() {
        let sections = filter(&help_sections(&keymap("")), "fold");
        let expected = [
            "Trees (lists pane and tasks pane)",
            "h, left",
            "l, right",
            "za",
            "zM",
            "zR",
            "dd",
            "Lists pane",
            "enter",
            "o",
            "O",
            "r",
        ];

        let system_under_test: Vec<String> = rows(&sections).map(row_label).collect();

        assert_eq!(system_under_test, expected);
    }

    #[test]
    fn row_keys_identify_headings_and_entries() {
        let sections = filter(&help_sections(&keymap("")), "zM");
        let expected = [
            HelpRowKey::Heading(HelpHeading::Trees),
            HelpRowKey::Entry(HelpSubject::Action(Action::FoldAll)),
        ];

        let system_under_test: Vec<HelpRowKey> = rows(&sections).map(HelpRow::key).collect();

        assert_eq!(system_under_test, expected);
    }

    #[test]
    fn row_key_is_a_top_level_outline_item_of_itself() {
        let row_key = HelpRowKey::Heading(HelpHeading::TasksPane);

        let system_under_test = row_key;

        assert_eq!(OutlineItem::key(&system_under_test), &row_key);
        assert_eq!(system_under_test.depth(), 0);
    }

    #[test]
    fn rows_of_no_sections_are_empty() {
        let sections = Vec::new();

        let system_under_test = rows(&sections).count();

        assert_eq!(system_under_test, 0);
    }
}

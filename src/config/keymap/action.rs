//! The registry of everything a user can do: [`Action`], the [`Context`]s it is valid in and
//! its default keys.
//!
//! Every property of an action is defined by one exhaustive `match`, so adding an action
//! fails to compile until its description, label, contexts and default keys are defined.

use std::fmt::{self, Display, Formatter};

use serde::Deserialize;

use crate::config::keymap::key::KeySequence;

/// Where the user is when a key is pressed; bindings are looked up per context.
///
/// Displays as the place a user would name, such as `tasks pane`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, strum::EnumIter)]
#[non_exhaustive]
pub enum Context {
    /// The lists pane is focused.
    Lists,
    /// The tasks pane is focused.
    Tasks,
    /// Several rows of the focused pane are selected in visual mode.
    Visual,
    /// The help screen is open.
    Help,
    /// A confirmation dialog is open.
    Dialog,
    /// A text input is open.
    Input,
}

/// Something a user can trigger with a key.
///
/// The kebab-case names (`move-down`, `new-task-below`) are user-facing: they are the keys of
/// the `keybindings` section of the configuration file, and an action displays as its name.
///
/// # Examples
///
/// ```
/// use rudo::config::keymap::action::{Action, Context};
///
/// let action = Action::Quit;
/// assert_eq!(action.contexts(), [Context::Lists, Context::Tasks]);
/// let keys: Vec<String> = action.default_keys().iter().map(ToString::to_string).collect();
/// assert_eq!(keys, ["q", "ctrl-c"]);
/// ```
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    Deserialize,
    strum::Display,
    strum::EnumIter,
    strum::IntoStaticStr,
)]
#[serde(rename_all = "kebab-case")]
#[strum(serialize_all = "kebab-case")]
#[non_exhaustive]
pub enum Action {
    /// Open the help screen.
    Help,
    /// Quit RuDo.
    Quit,
    /// Focus the next pane.
    FocusNext,
    /// Focus the previous pane.
    FocusPrevious,
    /// Focus the lists pane.
    FocusLists,
    /// Focus the tasks pane.
    FocusTasks,
    /// Filter the focused pane.
    Search,
    /// Remove the active filter.
    ClearFilter,
    /// Undo the last change to a task.
    Undo,
    /// Redo the last undone change.
    Redo,
    /// Open the list in the editor at the selected task.
    OpenInEditor,
    /// Select the next row.
    MoveDown,
    /// Select the previous row.
    MoveUp,
    /// Select the first row.
    MoveToTop,
    /// Select the last row.
    MoveToBottom,
    /// Move half a page down.
    HalfPageDown,
    /// Move half a page up.
    HalfPageUp,
    /// Fold the selected entry, else go to its parent.
    Collapse,
    /// Unfold the selected entry, else open the selected list.
    Expand,
    /// Fold or unfold the selected entry.
    ToggleFold,
    /// Fold everything.
    FoldAll,
    /// Unfold everything.
    UnfoldAll,
    /// Open the selected list, or fold or unfold a folder.
    Open,
    /// Create a list.
    NewList,
    /// Create a folder.
    NewFolder,
    /// Rename the selected list or folder.
    Rename,
    /// Move the selected lists and folders into another folder.
    MoveToFolder,
    /// Show or hide hidden folders and lists.
    ToggleHidden,
    /// Delete the selected list, folder or task.
    Delete,
    /// Add a task below the selected one.
    NewTaskBelow,
    /// Add a task above the selected one.
    NewTaskAbove,
    /// Add a subtask to the selected task.
    NewSubtask,
    /// Edit the task text.
    Edit,
    /// Mark the selected task as done or open again.
    ToggleDone,
    /// Copy the selected task and its subtasks.
    Yank,
    /// Paste below the selected task.
    PasteBelow,
    /// Paste above the selected task.
    PasteAbove,
    /// Swap the selected task with the next sibling.
    MoveTaskDown,
    /// Swap the selected task with the previous sibling.
    MoveTaskUp,
    /// Make the selected task a subtask of the task above.
    Indent,
    /// Move the selected task one level up.
    Outdent,
    /// Raise the priority.
    PriorityUp,
    /// Lower the priority.
    PriorityDown,
    /// Set or clear the due date.
    SetDueDate,
    /// Set or clear the recurrence.
    SetRecurrence,
    /// Sort the selected task and its siblings by priority.
    SortByPriority,
    /// Sort by due date.
    SortByDueDate,
    /// Sort by text.
    SortAlphabetically,
    /// Sort by status.
    SortByStatus,
    /// Show or hide completed tasks.
    ToggleCompleted,
    /// Start or end selecting several rows.
    VisualMode,
    /// Delete the selected tasks, lists or empty folders.
    DeleteSelection,
    /// Copy the selected tasks and their subtasks.
    YankSelection,
    /// Close the help screen.
    CloseHelp,
    /// Confirm a dialog.
    ConfirmYes,
    /// Cancel a dialog.
    ConfirmNo,
    /// Accept the typed text.
    Submit,
    /// Discard the typed text or end the selection.
    Cancel,
}

impl Display for Context {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Lists => "lists pane",
            Self::Tasks => "tasks pane",
            Self::Visual => "visual mode",
            Self::Help => "help screen",
            Self::Dialog => "dialogs",
            Self::Input => "text input",
        })
    }
}

const GLOBAL: &[Context] = &[Context::Lists, Context::Tasks];
const SEARCH: &[Context] = &[Context::Lists, Context::Tasks, Context::Help];
const NAVIGATION: &[Context] = &[
    Context::Lists,
    Context::Tasks,
    Context::Visual,
    Context::Help,
];
const TREE: &[Context] = &[Context::Lists, Context::Tasks];
const LISTS: &[Context] = &[Context::Lists];
const LISTS_AND_VISUAL: &[Context] = &[Context::Lists, Context::Visual];
const TASKS: &[Context] = &[Context::Tasks];
const PANES_AND_VISUAL: &[Context] = &[Context::Lists, Context::Tasks, Context::Visual];
const VISUAL: &[Context] = &[Context::Visual];
const HELP: &[Context] = &[Context::Help];
const DIALOG: &[Context] = &[Context::Dialog];
const INPUT_AND_VISUAL: &[Context] = &[Context::Input, Context::Visual];
const INPUT: &[Context] = &[Context::Input];

impl Action {
    /// Returns the sentence shown on the help screen, lowercase and without a trailing period.
    #[must_use]
    pub const fn description(self) -> &'static str {
        match self {
            Self::Help => "open the help screen",
            Self::Quit => "quit RuDo",
            Self::FocusNext => "focus the next pane",
            Self::FocusPrevious => "focus the previous pane (`shift-tab`)",
            Self::FocusLists => "focus the lists pane",
            Self::FocusTasks => "focus the tasks pane",
            Self::Search => "filter the focused pane (also inside the help)",
            Self::ClearFilter => "remove the active filter",
            Self::Undo => "undo the last change to a task",
            Self::Redo => "redo the last undone change",
            Self::ToggleCompleted => "show or hide completed tasks",
            Self::OpenInEditor => "open the list in your editor at the selected task",
            Self::MoveDown => "next row",
            Self::MoveUp => "previous row",
            Self::MoveToTop => "first row",
            Self::MoveToBottom => "last row",
            Self::HalfPageDown => "half a page down",
            Self::HalfPageUp => "half a page up",
            Self::Collapse => {
                "fold, else go to the parent; from a top-level task go to the lists pane"
            }
            Self::Expand => "unfold, else open the selected list",
            Self::ToggleFold => "fold or unfold the selected entry",
            Self::FoldAll => "fold everything",
            Self::UnfoldAll => "unfold everything",
            Self::Open => "open the selected list, or fold or unfold a folder",
            Self::NewList => "create a list next to the selection or inside the folder",
            Self::NewFolder => "create a folder",
            Self::Rename => "rename the selected list or folder",
            Self::MoveToFolder => "move the list, folder or selection into a folder",
            Self::ToggleHidden => "show or hide hidden folders and lists",
            Self::Delete => "delete the selected list, empty folder or task",
            Self::NewTaskBelow => "add a task below the selected one",
            Self::NewTaskAbove => "add a task above the selected one",
            Self::NewSubtask => "add a subtask to the selected task",
            Self::Edit => "edit the task text",
            Self::ToggleDone => "mark as done or open again",
            Self::Yank => "copy the task and its subtasks",
            Self::PasteBelow => "paste below the selected task",
            Self::PasteAbove => "paste above the selected task",
            Self::MoveTaskDown => "swap with the next sibling",
            Self::MoveTaskUp => "swap with the previous sibling",
            Self::Indent => "make it a subtask of the task above",
            Self::Outdent => "move it one level up",
            Self::PriorityUp => "raise the priority",
            Self::PriorityDown => "lower the priority",
            Self::SetDueDate => "set or clear the due date",
            Self::SetRecurrence => "set or clear the recurrence",
            Self::SortByPriority => "sort the selected task and its siblings",
            Self::SortByDueDate => "sort by due date, tasks without one last",
            Self::SortAlphabetically => "sort by text",
            Self::SortByStatus => "open tasks first",
            Self::VisualMode => "select several rows from here on, or stop selecting",
            Self::DeleteSelection => "delete the selected tasks, lists or empty folders",
            Self::YankSelection => "copy the selected tasks and their subtasks",
            Self::CloseHelp => "close the help screen",
            Self::ConfirmYes => "confirm",
            Self::ConfirmNo => "cancel",
            Self::Submit => "accept the text",
            Self::Cancel => "discard the text, or end the selection",
        }
    }

    /// Returns the few words shown next to the key in the bottom bar, such as `new task`.
    #[must_use]
    pub const fn short_label(self) -> &'static str {
        match self {
            Self::Help => "help",
            Self::Quit => "quit",
            Self::FocusNext => "next pane",
            Self::FocusPrevious => "previous pane",
            Self::FocusLists => "lists",
            Self::FocusTasks => "tasks",
            Self::Search => "search",
            Self::ClearFilter => "clear filter",
            Self::Undo => "undo",
            Self::Redo => "redo",
            Self::ToggleCompleted => "completed",
            Self::OpenInEditor => "editor",
            Self::MoveDown => "down",
            Self::MoveUp => "up",
            Self::MoveToTop => "top",
            Self::MoveToBottom => "bottom",
            Self::HalfPageDown => "page down",
            Self::HalfPageUp => "page up",
            Self::Collapse => "collapse",
            Self::Expand => "expand",
            Self::ToggleFold => "fold",
            Self::FoldAll => "fold all",
            Self::UnfoldAll => "unfold all",
            Self::Open => "open",
            Self::NewList => "new list",
            Self::NewFolder => "new folder",
            Self::Rename => "rename",
            Self::MoveToFolder => "move",
            Self::ToggleHidden => "hidden",
            Self::Delete | Self::DeleteSelection => "delete",
            Self::NewTaskBelow => "new task",
            Self::NewTaskAbove => "task above",
            Self::NewSubtask => "subtask",
            Self::Edit => "edit",
            Self::ToggleDone => "done",
            Self::Yank | Self::YankSelection => "yank",
            Self::PasteBelow => "paste",
            Self::PasteAbove => "paste above",
            Self::MoveTaskDown => "move down",
            Self::MoveTaskUp => "move up",
            Self::Indent => "indent",
            Self::Outdent => "outdent",
            Self::PriorityUp => "priority up",
            Self::PriorityDown => "priority down",
            Self::SetDueDate => "due date",
            Self::SetRecurrence => "repeat",
            Self::SortByPriority => "sort priority",
            Self::SortByDueDate => "sort due",
            Self::SortAlphabetically => "sort a-z",
            Self::SortByStatus => "sort status",
            Self::VisualMode => "visual",
            Self::CloseHelp => "close",
            Self::ConfirmYes => "yes",
            Self::ConfirmNo => "no",
            Self::Submit => "accept",
            Self::Cancel => "cancel",
        }
    }

    /// Returns the contexts in which the action can be triggered; never empty.
    ///
    /// Global actions are valid in the lists and tasks pane, navigation actions also in
    /// visual mode and on the help screen.
    #[must_use]
    pub const fn contexts(self) -> &'static [Context] {
        match self {
            Self::Help
            | Self::Quit
            | Self::FocusNext
            | Self::FocusPrevious
            | Self::FocusLists
            | Self::FocusTasks
            | Self::ClearFilter
            | Self::Undo
            | Self::Redo
            | Self::OpenInEditor => GLOBAL,
            Self::Search => SEARCH,
            Self::MoveDown
            | Self::MoveUp
            | Self::MoveToTop
            | Self::MoveToBottom
            | Self::HalfPageDown
            | Self::HalfPageUp => NAVIGATION,
            Self::Collapse
            | Self::Expand
            | Self::ToggleFold
            | Self::FoldAll
            | Self::UnfoldAll
            | Self::Delete => TREE,
            Self::Open | Self::NewList | Self::NewFolder | Self::Rename | Self::ToggleHidden => {
                LISTS
            }
            Self::MoveToFolder => LISTS_AND_VISUAL,
            Self::NewTaskBelow
            | Self::NewTaskAbove
            | Self::NewSubtask
            | Self::Edit
            | Self::ToggleDone
            | Self::Yank
            | Self::PasteBelow
            | Self::PasteAbove
            | Self::MoveTaskDown
            | Self::MoveTaskUp
            | Self::Indent
            | Self::Outdent
            | Self::PriorityUp
            | Self::PriorityDown
            | Self::SetDueDate
            | Self::SetRecurrence
            | Self::SortByPriority
            | Self::SortByDueDate
            | Self::SortAlphabetically
            | Self::SortByStatus
            | Self::ToggleCompleted => TASKS,
            Self::VisualMode => PANES_AND_VISUAL,
            Self::DeleteSelection | Self::YankSelection => VISUAL,
            Self::CloseHelp => HELP,
            Self::ConfirmYes | Self::ConfirmNo => DIALOG,
            Self::Submit => INPUT,
            Self::Cancel => INPUT_AND_VISUAL,
        }
    }

    /// Returns the keys bound to the action when the configuration does not override them.
    ///
    /// # Panics
    ///
    /// Never panics in practice: the default notation is a constant that the tests parse for
    /// every action.
    #[must_use]
    pub fn default_keys(self) -> Vec<KeySequence> {
        self.default_notation()
            .iter()
            .map(|notation| notation.parse().expect("default keys are valid notation"))
            .collect()
    }

    const fn default_notation(self) -> &'static [&'static str] {
        match self {
            Self::Help => &["?"],
            Self::Quit => &["q", "ctrl-c"],
            Self::FocusNext => &["tab"],
            Self::FocusPrevious => &["backtab"],
            Self::FocusLists => &["1"],
            Self::FocusTasks => &["2"],
            Self::Search => &["/"],
            Self::ClearFilter => &["esc"],
            Self::Undo => &["u"],
            Self::Redo => &["ctrl-r"],
            Self::ToggleCompleted | Self::ToggleHidden => &["H"],
            Self::OpenInEditor => &["e"],
            Self::MoveDown => &["j", "down"],
            Self::MoveUp => &["k", "up"],
            Self::MoveToTop => &["g g", "home"],
            Self::MoveToBottom => &["G", "end"],
            Self::HalfPageDown => &["ctrl-d", "pagedown"],
            Self::HalfPageUp => &["ctrl-u", "pageup"],
            Self::Collapse => &["h", "left"],
            Self::Expand => &["l", "right"],
            Self::ToggleFold => &["z a"],
            Self::FoldAll => &["z M"],
            Self::UnfoldAll => &["z R"],
            Self::Open | Self::Submit => &["enter"],
            Self::NewList | Self::NewTaskBelow => &["o"],
            Self::NewFolder | Self::NewTaskAbove => &["O"],
            Self::Rename | Self::SetRecurrence => &["r"],
            Self::MoveToFolder => &["m"],
            Self::VisualMode => &["v"],
            Self::DeleteSelection => &["d"],
            Self::YankSelection => &["y"],
            Self::Delete => &["d d"],
            Self::NewSubtask => &["a"],
            Self::Edit => &["i", "enter"],
            Self::ToggleDone => &["x", "space"],
            Self::Yank => &["y y"],
            Self::PasteBelow => &["p"],
            Self::PasteAbove => &["P"],
            Self::MoveTaskDown => &["J"],
            Self::MoveTaskUp => &["K"],
            Self::Indent => &[">"],
            Self::Outdent => &["<"],
            Self::PriorityUp => &["+"],
            Self::PriorityDown => &["-"],
            Self::SetDueDate => &["D"],
            Self::SortByPriority => &["s p"],
            Self::SortByDueDate => &["s d"],
            Self::SortAlphabetically => &["s a"],
            Self::SortByStatus => &["s s"],
            Self::CloseHelp => &["?", "q", "esc"],
            Self::ConfirmYes => &["y", "enter"],
            Self::ConfirmNo => &["n", "esc"],
            Self::Cancel => &["esc", "ctrl-c"],
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use rstest::rstest;
    use strum::IntoEnumIterator;

    use super::*;
    use crate::readme;

    fn sequences(notations: &[&str]) -> Vec<KeySequence> {
        notations
            .iter()
            .map(|notation| notation.parse().unwrap())
            .collect()
    }

    fn parse_action(name: &str) -> Action {
        serde_saphyr::from_str(name).unwrap()
    }

    /// Returns every row of the README keybinding tables as the action with its keys, in table
    /// order.
    fn readme_bindings() -> Vec<(Action, Vec<KeySequence>)> {
        readme::table_rows(&readme::section("## Keybindings"), "Keys")
            .iter()
            .map(|cells| {
                let keys = readme::code_spans(cells[0])
                    .map(|notation| notation.parse().unwrap())
                    .collect();
                let [action] = readme::code_spans(cells[1]).collect::<Vec<_>>()[..] else {
                    panic!("expected one action in {cells:?}");
                };
                (parse_action(action), keys)
            })
            .collect()
    }

    #[test]
    fn readme_keybinding_tables_list_every_action_in_registry_order() {
        let expected: Vec<Action> = Action::iter().collect();
        let mut seen = HashSet::new();

        let system_under_test: Vec<Action> = readme_bindings()
            .into_iter()
            .map(|(action, _)| action)
            .filter(|action| seen.insert(*action))
            .collect();

        assert_eq!(system_under_test, expected);
    }

    #[test]
    fn readme_keybinding_tables_show_the_default_keys() {
        let expected: Vec<(Action, Vec<KeySequence>)> = readme_bindings()
            .into_iter()
            .map(|(action, _)| (action, action.default_keys()))
            .collect();

        let system_under_test = readme_bindings();

        assert_eq!(system_under_test, expected);
    }

    #[rstest]
    #[case::quit(Action::Quit, &["q", "ctrl-c"])]
    #[case::move_down(Action::MoveDown, &["j", "down"])]
    #[case::move_to_top(Action::MoveToTop, &["g g", "home"])]
    #[case::delete(Action::Delete, &["d d"])]
    #[case::toggle_done(Action::ToggleDone, &["x", "space"])]
    #[case::cancel(Action::Cancel, &["esc", "ctrl-c"])]
    #[case::visual_mode(Action::VisualMode, &["v"])]
    #[case::delete_selection(Action::DeleteSelection, &["d"])]
    #[case::yank_selection(Action::YankSelection, &["y"])]
    #[case::move_to_folder(Action::MoveToFolder, &["m"])]
    #[case::toggle_completed(Action::ToggleCompleted, &["H"])]
    #[case::toggle_hidden(Action::ToggleHidden, &["H"])]
    fn default_keys_match_the_readme(#[case] action: Action, #[case] notations: &[&str]) {
        let expected = sequences(notations);

        let system_under_test = action.default_keys();

        assert_eq!(system_under_test, expected);
    }

    #[rstest]
    #[case::global(Action::Quit, &[Context::Lists, Context::Tasks])]
    #[case::search(Action::Search, &[Context::Lists, Context::Tasks, Context::Help])]
    #[case::navigation(
        Action::MoveDown,
        &[Context::Lists, Context::Tasks, Context::Visual, Context::Help]
    )]
    #[case::tree(Action::ToggleFold, &[Context::Lists, Context::Tasks])]
    #[case::delete(Action::Delete, &[Context::Lists, Context::Tasks])]
    #[case::lists(Action::NewList, &[Context::Lists])]
    #[case::tasks(Action::NewTaskBelow, &[Context::Tasks])]
    #[case::toggle_completed(Action::ToggleCompleted, &[Context::Tasks])]
    #[case::toggle_hidden(Action::ToggleHidden, &[Context::Lists])]
    #[case::help(Action::CloseHelp, &[Context::Help])]
    #[case::dialog(Action::ConfirmYes, &[Context::Dialog])]
    #[case::input(Action::Submit, &[Context::Input])]
    #[case::visual_mode(Action::VisualMode, &[Context::Lists, Context::Tasks, Context::Visual])]
    #[case::selection(Action::DeleteSelection, &[Context::Visual])]
    #[case::move_to_folder(Action::MoveToFolder, &[Context::Lists, Context::Visual])]
    #[case::cancel(Action::Cancel, &[Context::Input, Context::Visual])]
    fn contexts_follow_the_readme_sections(#[case] action: Action, #[case] expected: &[Context]) {
        let system_under_test = action.contexts();

        assert_eq!(system_under_test, expected);
    }

    #[rstest]
    #[case::help("help", Action::Help)]
    #[case::move_down("move-down", Action::MoveDown)]
    #[case::new_task_below("new-task-below", Action::NewTaskBelow)]
    #[case::open_in_editor("open-in-editor", Action::OpenInEditor)]
    fn deserializes_kebab_case_names(#[case] name: &str, #[case] expected: Action) {
        let system_under_test = parse_action(name);

        assert_eq!(system_under_test, expected);
    }

    #[test]
    fn every_action_displays_as_its_configuration_name() {
        for action in Action::iter() {
            let name = action.to_string();

            let system_under_test = parse_action(&name);

            assert_eq!(system_under_test, action, "{name}");
        }
    }

    #[rstest]
    #[case::lists(Context::Lists, "lists pane")]
    #[case::tasks(Context::Tasks, "tasks pane")]
    #[case::visual(Context::Visual, "visual mode")]
    #[case::help(Context::Help, "help screen")]
    #[case::dialog(Context::Dialog, "dialogs")]
    #[case::input(Context::Input, "text input")]
    fn context_displays_as_a_place(#[case] context: Context, #[case] expected: &str) {
        let system_under_test = context.to_string();

        assert_eq!(system_under_test, expected);
    }

    #[rstest]
    #[case::pascal_case("MoveDown")]
    #[case::snake_case("move_down")]
    #[case::unknown("teleport")]
    fn rejects_names_that_are_not_kebab_case_actions(#[case] name: &str) {
        let system_under_test = serde_saphyr::from_str::<Action>(name);

        assert!(system_under_test.is_err(), "{name} was accepted");
    }

    #[test]
    fn every_action_has_distinct_contexts() {
        for action in Action::iter() {
            let contexts = action.contexts();

            let system_under_test: HashSet<&Context> = contexts.iter().collect();

            assert!(!contexts.is_empty(), "{action:?} has no context");
            assert_eq!(system_under_test.len(), contexts.len(), "{action:?}");
        }
    }

    #[test]
    fn every_context_has_an_action() {
        for context in Context::iter() {
            let mut actions = Action::iter();

            let system_under_test = actions.any(|action| action.contexts().contains(&context));

            assert!(system_under_test, "{context:?} has no action");
        }
    }

    #[test]
    fn every_description_is_a_lowercase_phrase_without_period() {
        for action in Action::iter() {
            let system_under_test = action.description();

            assert!(
                system_under_test.starts_with(|first: char| first.is_lowercase()),
                "{action:?}: {system_under_test}"
            );
            assert!(!system_under_test.ends_with('.'), "{action:?}");
        }
    }

    #[test]
    fn every_short_label_is_a_few_lowercase_words() {
        for action in Action::iter() {
            let system_under_test = action.short_label();

            assert!(!system_under_test.is_empty(), "{action:?}");
            assert_eq!(system_under_test, system_under_test.to_lowercase());
            assert!(system_under_test.len() <= 16, "{action:?}");
        }
    }

    #[test]
    fn every_action_has_parseable_default_keys() {
        for action in Action::iter() {
            let system_under_test = action.default_keys();

            assert!(
                !system_under_test.is_empty(),
                "{action:?} has no default key"
            );
        }
    }
}

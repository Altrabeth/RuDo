//! The bottom bar: key hints, pending keys and messages.
//!
//! The left part shows the most important keys of the focused pane, the [`StatusMessage`]
//! while one is visible, or the search prompt, vim style, while a query is typed. In visual
//! mode the hints follow [`VISUAL_INDICATOR`] and the number of selected rows. The right
//! part always shows the key of the help
//! screen and, like vim's `showcmd`, the keys typed so far of an unfinished key sequence.

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::Block;

use crate::app::search::PROMPT;
use crate::app::{App, Focus, Severity, StatusMessage};
use crate::config::keymap::action::{Action, Context};
use crate::config::keymap::key::{KeyChord, KeySequence};
use crate::config::keymap::keymap::Keymap;
use crate::config::theme::{Slot, Theme};
use crate::ui::text_input::{TextInput, TextInputView};

/// The spans of one key hint: a blank, the key, a blank, the label and a blank, so that
/// consecutive hints are two blanks apart.
pub(crate) type KeyHint = [Span<'static>; 5];

/// Starts the bottom bar in visual mode, vim style, followed by the number of selected rows.
pub const VISUAL_INDICATOR: &str = "-- VISUAL --";

/// Renders the bottom bar of `app` into `area`, a single line below the panes.
///
/// Key hints that do not fit are dropped from the right; the pinned hint of the context (the
/// help key in the panes) and the pending keys are always shown.
pub fn render(app: &App, frame: &mut Frame, area: Rect) {
    let theme = app.theme();
    let pinned =
        pinned_action(app.context()).and_then(|action| key_hint(app.keymap(), action, theme));
    let pending = pending_keys(app.pending_keys(), theme);
    let right = Line::from_iter(
        pinned
            .into_iter()
            .flatten()
            .chain(pending.into_iter().flatten()),
    );
    let right_width = u16::try_from(right.width()).unwrap_or(u16::MAX);
    let [left_area, right_area] =
        Layout::horizontal([Constraint::Fill(1), Constraint::Length(right_width)]).areas(area);
    frame.render_widget(Block::new().style(theme.style(Slot::StatusBar)), area);
    frame.render_widget(right, right_area);
    if let Some(input) = app.search_input() {
        render_search_prompt(input, theme, frame, left_area);
        return;
    }
    let left = if let Some(message) = app.status_message() {
        message_line(message, theme)
    } else {
        let mut line = visual_line(app.visual_selection_count(), theme);
        let hints = hints_line(
            app.keymap(),
            hint_actions(app.context(), app.focus()),
            usize::from(left_area.width).saturating_sub(line.width()),
            theme,
        );
        line.spans.extend(hints.spans);
        line
    };
    frame.render_widget(left, left_area);
}

/// Renders the search prompt into `area`: a blank, the [`PROMPT`] and the typed query with
/// the cursor.
fn render_search_prompt(input: &TextInput, theme: &Theme, frame: &mut Frame, area: Rect) {
    let prompt = Line::from_iter([
        Span::raw(" "),
        Span::styled(PROMPT, theme.style(Slot::KeyHint)),
    ]);
    let prompt_width = u16::try_from(prompt.width()).unwrap_or(u16::MAX);
    let [prompt_area, input_area] =
        Layout::horizontal([Constraint::Length(prompt_width), Constraint::Fill(1)]).areas(area);
    frame.render_widget(prompt, prompt_area);
    TextInputView::new(input, "")
        .style(theme.style(Slot::StatusBar))
        .render_with_cursor(frame, input_area);
}

/// Returns ` -- VISUAL -- N selected` while `selected` rows are selected in visual mode, and
/// an empty line otherwise.
fn visual_line(selected: Option<usize>, theme: &Theme) -> Line<'static> {
    let Some(selected) = selected else {
        return Line::default();
    };
    Line::from_iter([
        Span::raw(" "),
        Span::styled(VISUAL_INDICATOR, theme.style(Slot::KeyHint)),
        Span::styled(
            format!(" {selected} selected "),
            theme.style(Slot::KeyHintLabel),
        ),
    ])
}

/// Returns the actions whose keys the bottom bar shows in `context`, most important first;
/// in visual mode those that act on the selection of the `focus`ed pane.
const fn hint_actions(context: Context, focus: Focus) -> &'static [Action] {
    match context {
        Context::Lists => &[
            Action::NewList,
            Action::NewFolder,
            Action::Open,
            Action::Rename,
            Action::Delete,
        ],
        Context::Tasks => &[
            Action::NewTaskBelow,
            Action::NewSubtask,
            Action::Edit,
            Action::ToggleDone,
            Action::Delete,
            Action::SetDueDate,
        ],
        Context::Visual => match focus {
            Focus::Lists => &[
                Action::DeleteSelection,
                Action::MoveToFolder,
                Action::Cancel,
            ],
            Focus::Tasks => &[
                Action::DeleteSelection,
                Action::YankSelection,
                Action::Cancel,
            ],
        },
        Context::Bin => &[
            Action::RestoreFromBin,
            Action::Delete,
            Action::EmptyBin,
            Action::CloseBin,
        ],
        Context::BinVisual => &[
            Action::RestoreFromBin,
            Action::DeleteSelection,
            Action::Cancel,
        ],
        Context::Help => &[Action::Search],
        Context::Dialog => &[Action::ConfirmYes, Action::ConfirmNo],
        Context::Input => &[Action::Submit, Action::Cancel],
        Context::Picker => &[Action::Complete, Action::Submit, Action::Cancel],
    }
}

/// Returns the action whose key the bottom bar always shows at its right end in `context`:
/// how to open the help in the panes and how to leave it in the help screen. Visual mode,
/// dialogs and text inputs pin nothing, because there `?` is not the help key.
const fn pinned_action(context: Context) -> Option<Action> {
    match context {
        Context::Lists | Context::Tasks | Context::Bin => Some(Action::Help),
        Context::Help => Some(Action::CloseHelp),
        Context::Visual
        | Context::BinVisual
        | Context::Dialog
        | Context::Input
        | Context::Picker => None,
    }
}

/// Returns the hints of `actions` that fit into `max_width` columns, dropping the rest from
/// the right.
fn hints_line(
    keymap: &Keymap,
    actions: &[Action],
    max_width: usize,
    theme: &Theme,
) -> Line<'static> {
    let mut used_width = 0;
    let spans = actions
        .iter()
        .filter_map(|&action| key_hint(keymap, action, theme))
        .take_while(|hint| {
            used_width += hint.iter().map(Span::width).sum::<usize>();
            used_width <= max_width
        })
        .flatten();
    Line::from_iter(spans)
}

/// Returns the hint `<key> <short label>` for `action` with its first key, or `None` if the
/// action is unbound.
pub(crate) fn key_hint(keymap: &Keymap, action: Action, theme: &Theme) -> Option<KeyHint> {
    let keys = keymap.keys(action).first()?;
    Some([
        Span::raw(" "),
        Span::styled(keys.compact().to_string(), theme.style(Slot::KeyHint)),
        Span::raw(" "),
        Span::styled(action.short_label(), theme.style(Slot::KeyHintLabel)),
        Span::raw(" "),
    ])
}

/// Returns the `pending` keys in their compact form, or `None` if no keys are pending.
fn pending_keys(pending: &[KeyChord], theme: &Theme) -> Option<[Span<'static>; 2]> {
    // The only error is an empty sequence, which means that no keys are pending.
    let keys = KeySequence::new(pending.to_vec()).ok()?;
    Some([
        Span::styled(keys.compact().to_string(), theme.style(Slot::PendingKeys)),
        Span::raw(" "),
    ])
}

/// Returns `message` in the colour of its severity.
fn message_line<'message>(message: &'message StatusMessage, theme: &Theme) -> Line<'message> {
    let slot = match message.severity() {
        Severity::Info => Slot::MessageInfo,
        Severity::Error => Slot::MessageError,
    };
    Line::from_iter([
        Span::raw(" "),
        Span::styled(message.text(), theme.style(slot)),
    ])
}

#[cfg(test)]
mod tests {
    use rstest::rstest;
    use strum::IntoEnumIterator;

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

    fn chords(notation: &str) -> Vec<KeyChord> {
        notation
            .split_whitespace()
            .map(|chord| chord.parse().unwrap())
            .collect()
    }

    #[rstest]
    #[case::defaults("", " o new list  O new folder  enter open  r rename  dd delete ")]
    #[case::remapped(
        "new-list: [w, o]",
        " w new list  O new folder  enter open  r rename  dd delete "
    )]
    #[case::unbound("new-folder: []", " o new list  enter open  r rename  dd delete ")]
    #[case::sequence(
        "rename: [c w]",
        " o new list  O new folder  enter open  cw rename  dd delete "
    )]
    #[case::named_key(
        "rename: [ctrl-n]",
        " o new list  O new folder  enter open  ctrl-n rename  dd delete "
    )]
    fn hints_follow_the_keymap(#[case] keybindings: &str, #[case] expected: &str) {
        let keymap = keymap(keybindings);

        let system_under_test = hints_line(
            &keymap,
            hint_actions(Context::Lists, Focus::Lists),
            100,
            &Theme::default(),
        );

        assert_eq!(system_under_test.to_string(), expected);
    }

    #[rstest]
    #[case::exact_fit(23, " o new task  a subtask ")]
    #[case::one_column_short(22, " o new task ")]
    #[case::nothing_fits(11, "")]
    fn hints_that_do_not_fit_are_dropped_from_the_right(
        #[case] max_width: usize,
        #[case] expected: &str,
    ) {
        let keymap = keymap("");

        let system_under_test = hints_line(
            &keymap,
            hint_actions(Context::Tasks, Focus::Tasks),
            max_width,
            &Theme::default(),
        );

        assert_eq!(system_under_test.to_string(), expected);
    }

    #[test]
    fn hint_width_counts_display_columns() {
        let keymap = keymap("new-task-below: [ü]");
        let expected = " ü new task ";

        let system_under_test = hints_line(
            &keymap,
            hint_actions(Context::Tasks, Focus::Tasks),
            expected.chars().count(),
            &Theme::default(),
        );

        assert_eq!(system_under_test.to_string(), expected);
    }

    #[test]
    fn every_hint_action_is_valid_in_its_context() {
        let contexts: Vec<Context> = Context::iter().collect();

        let system_under_test: Vec<(Context, Action)> = contexts
            .iter()
            .flat_map(|&context| {
                Focus::iter().flat_map(move |focus| {
                    hint_actions(context, focus)
                        .iter()
                        .map(move |&action| (context, action))
                })
            })
            .filter(|(context, action)| !action.contexts().contains(context))
            .collect();

        assert_eq!(system_under_test, []);
    }

    #[test]
    fn every_pinned_action_is_valid_in_its_context() {
        let contexts: Vec<Context> = Context::iter().collect();

        let system_under_test: Vec<(Context, Action)> = contexts
            .iter()
            .filter_map(|&context| pinned_action(context).map(|action| (context, action)))
            .filter(|(context, action)| !action.contexts().contains(context))
            .collect();

        assert_eq!(system_under_test, []);
    }

    #[rstest]
    #[case::lists(Context::Lists, Some(Action::Help))]
    #[case::tasks(Context::Tasks, Some(Action::Help))]
    #[case::help(Context::Help, Some(Action::CloseHelp))]
    #[case::visual(Context::Visual, None)]
    #[case::bin(Context::Bin, Some(Action::Help))]
    #[case::bin_visual(Context::BinVisual, None)]
    #[case::dialog(Context::Dialog, None)]
    #[case::input(Context::Input, None)]
    #[case::picker(Context::Picker, None)]
    fn pinned_action_says_how_to_open_or_leave_the_help(
        #[case] context: Context,
        #[case] expected: Option<Action>,
    ) {
        let system_under_test = pinned_action(context);

        assert_eq!(system_under_test, expected);
    }

    #[rstest]
    #[case::normal_mode(None, "")]
    #[case::one_row(Some(1), " -- VISUAL -- 1 selected ")]
    #[case::several_rows(Some(300), " -- VISUAL -- 300 selected ")]
    fn visual_line_counts_the_selected_rows(
        #[case] selected: Option<usize>,
        #[case] expected: &str,
    ) {
        let theme = Theme::default();

        let system_under_test = visual_line(selected, &theme);

        assert_eq!(system_under_test.to_string(), expected);
    }

    #[rstest]
    #[case::lists(
        Focus::Lists,
        &[Action::DeleteSelection, Action::MoveToFolder, Action::Cancel]
    )]
    #[case::tasks(
        Focus::Tasks,
        &[Action::DeleteSelection, Action::YankSelection, Action::Cancel]
    )]
    fn visual_hints_act_on_the_selection_of_the_focused_pane(
        #[case] focus: Focus,
        #[case] expected: &[Action],
    ) {
        let system_under_test = hint_actions(Context::Visual, focus);

        assert_eq!(system_under_test, expected);
    }

    #[test]
    fn visual_hints_in_the_bin_restore_or_delete_the_selection() {
        let expected = [
            Action::RestoreFromBin,
            Action::DeleteSelection,
            Action::Cancel,
        ];

        let system_under_test = hint_actions(Context::BinVisual, Focus::Lists);

        assert_eq!(system_under_test, expected);
    }

    #[rstest]
    #[case::none("", None)]
    #[case::one_character("z", Some("z "))]
    #[case::named_key("g home", Some("g home "))]
    fn pending_keys_are_shown_compactly(#[case] notation: &str, #[case] expected: Option<&str>) {
        let pending = chords(notation);

        let system_under_test = pending_keys(&pending, &Theme::default());

        assert_eq!(
            system_under_test.map(|spans| Line::from_iter(spans).to_string()),
            expected.map(str::to_owned)
        );
    }

    #[rstest]
    #[case::info(StatusMessage::info("list created"), Slot::MessageInfo)]
    #[case::error(StatusMessage::error("cannot save"), Slot::MessageError)]
    fn message_uses_the_slot_of_its_severity(
        #[case] message: StatusMessage,
        #[case] expected_slot: Slot,
    ) {
        let theme = Theme::default();
        let expected_text = format!(" {}", message.text());

        let system_under_test = message_line(&message, &theme);

        assert_eq!(system_under_test.to_string(), expected_text);
        assert_eq!(system_under_test.spans[1].style, theme.style(expected_slot));
    }
}

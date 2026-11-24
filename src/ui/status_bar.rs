//! The bottom bar: key hints, pending keys and messages.
//!
//! The left part shows the most important keys of the focused pane, or the
//! [`StatusMessage`] while one is visible. The right part always shows the key of the help
//! screen and, like vim's `showcmd`, the keys typed so far of an unfinished key sequence.

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::Block;

use crate::app::{App, Severity, StatusMessage};
use crate::config::keymap::action::{Action, Context};
use crate::config::keymap::key::{KeyChord, KeySequence};
use crate::config::keymap::keymap::Keymap;
use crate::config::theme::{Slot, Theme};

/// The spans of one key hint: a blank, the key, a blank, the label and a blank, so that
/// consecutive hints are two blanks apart.
pub(crate) type KeyHint = [Span<'static>; 5];

/// Renders the bottom bar of `app` into `area`, a single line below the panes.
///
/// Key hints that do not fit are dropped from the right; the help key and the pending keys
/// are always shown.
pub fn render(app: &App, frame: &mut Frame, area: Rect) {
    let theme = app.theme();
    let help = key_hint(app.keymap(), Action::Help, theme);
    let pending = pending_keys(app.pending_keys(), theme);
    let right = Line::from_iter(
        help.into_iter()
            .flatten()
            .chain(pending.into_iter().flatten()),
    );
    let right_width = u16::try_from(right.width()).unwrap_or(u16::MAX);
    let [left_area, right_area] =
        Layout::horizontal([Constraint::Fill(1), Constraint::Length(right_width)]).areas(area);
    let left = match app.status_message() {
        Some(message) => message_line(message, theme),
        None => hints_line(
            app.keymap(),
            app.context(),
            usize::from(left_area.width),
            theme,
        ),
    };
    frame.render_widget(Block::new().style(theme.style(Slot::StatusBar)), area);
    frame.render_widget(left, left_area);
    frame.render_widget(right, right_area);
}

/// Returns the actions whose keys the bottom bar shows in `context`, most important first.
const fn hint_actions(context: Context) -> &'static [Action] {
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
        Context::Help | Context::Dialog | Context::Input => &[],
    }
}

/// Returns the hints of `context` that fit into `max_width` columns, dropping the rest from
/// the right.
fn hints_line(keymap: &Keymap, context: Context, max_width: usize, theme: &Theme) -> Line<'static> {
    let mut used_width = 0;
    let spans = hint_actions(context)
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
        "new-list: [m, o]",
        " m new list  O new folder  enter open  r rename  dd delete "
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

        let system_under_test = hints_line(&keymap, Context::Lists, 100, &Theme::default());

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

        let system_under_test = hints_line(&keymap, Context::Tasks, max_width, &Theme::default());

        assert_eq!(system_under_test.to_string(), expected);
    }

    #[test]
    fn hint_width_counts_display_columns() {
        let keymap = keymap("new-task-below: [ü]");
        let expected = " ü new task ";

        let system_under_test = hints_line(
            &keymap,
            Context::Tasks,
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
                hint_actions(context)
                    .iter()
                    .map(move |&action| (context, action))
            })
            .filter(|(context, action)| !action.contexts().contains(context))
            .collect();

        assert_eq!(system_under_test, []);
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

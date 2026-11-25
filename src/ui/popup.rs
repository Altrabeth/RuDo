//! Popups drawn over the panes: the input popup that asks for text and the confirmation
//! dialog that asks yes or no.
//!
//! Both are centred by [`centered_rect`], have a fixed height, clear the cells below them and
//! use the `popup_border` and `popup_title` slots. They depend only on what they show, the
//! keymap and the theme, never on the application state. A [`Prompt`] holds the state of an
//! input popup; the feature that opens it decides what its title, hint, preview and error say.

use std::borrow::Cow;
use std::fmt::Write;

use ratatui::Frame;
use ratatui::buffer::Buffer;
use ratatui::layout::{Constraint, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Clear, Padding, Widget};

use crate::config::keymap::action::Action;
use crate::config::keymap::keymap::Keymap;
use crate::config::theme::{Slot, Theme};
use crate::ui::text_input::{TextInput, TextInputView};

/// Share of the screen width taken by a popup, within [`POPUP_MIN_WIDTH`] and the screen.
const POPUP_WIDTH_PERCENT: u32 = 60;
const POPUP_MIN_WIDTH: u16 = 30;
/// Columns of the borders and the padding on both sides of the content of a popup.
const POPUP_FRAME_WIDTH: u16 = 4;
/// Two border rows, the input, the hint, the preview and the error.
const INPUT_POPUP_HEIGHT: u16 = 6;
/// Two border rows, the message, an empty row and the keys.
const CONFIRM_DIALOG_HEIGHT: u16 = 5;
const CONFIRM_DIALOG_TITLE: &str = "Confirm";
/// The actions whose keys the confirmation dialog shows, in this order.
const CONFIRM_ACTIONS: [Action; 2] = [Action::ConfirmYes, Action::ConfirmNo];
/// Separates the keys of one action in the confirmation dialog, as in `y/enter`.
const KEY_SEPARATOR: char = '/';

/// Returns the area of a popup `height` rows high, centred in `area`.
///
/// The popup takes 60 % of the width, at least 30 columns; neither its width nor its height
/// exceeds `area`, so it fits on any terminal.
#[must_use]
pub fn centered_rect(area: Rect, height: u16) -> Rect {
    centered_rect_fitting(area, height, 0)
}

/// Returns the area of a popup `height` rows high, centred in `area`, like [`centered_rect`],
/// but wider if needed to show content `content_width` columns wide, as far as `area` allows.
fn centered_rect_fitting(area: Rect, height: u16, content_width: u16) -> Rect {
    let share = u32::from(area.width) * POPUP_WIDTH_PERCENT / 100;
    let width = u16::try_from(share)
        .unwrap_or(u16::MAX)
        .max(POPUP_MIN_WIDTH)
        .max(content_width.saturating_add(POPUP_FRAME_WIDTH))
        .min(area.width);
    area.centered(Constraint::Length(width), Constraint::Length(height))
}

/// The state of an input popup: its title, the edited text and optional lines below it.
///
/// The hint is set when the prompt opens, for example the accepted formats, and changes only
/// while completing a word, to list the candidates. The preview and the error change while
/// the user types, for example the date a text will be parsed as, or why it cannot be
/// submitted.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Prompt {
    title: Cow<'static, str>,
    input: TextInput,
    hint: Option<Cow<'static, str>>,
    preview: Option<Cow<'static, str>>,
    error: Option<Cow<'static, str>>,
}

impl Prompt {
    /// Creates a prompt titled `title` that edits `input`, without hint, preview or error.
    #[must_use]
    pub fn new(title: impl Into<Cow<'static, str>>, input: TextInput) -> Self {
        Self {
            title: title.into(),
            input,
            hint: None,
            preview: None,
            error: None,
        }
    }

    /// Adds a hint shown below the input.
    #[must_use]
    pub fn with_hint(mut self, hint: impl Into<Cow<'static, str>>) -> Self {
        self.hint = Some(hint.into());
        self
    }

    /// Shows `hint` below the input instead of the current hint.
    pub fn set_hint(&mut self, hint: impl Into<Cow<'static, str>>) {
        self.hint = Some(hint.into());
    }

    /// Returns the edited text and cursor.
    #[must_use]
    pub const fn input(&self) -> &TextInput {
        &self.input
    }

    /// Returns the edited text and cursor for editing.
    pub const fn input_mut(&mut self) -> &mut TextInput {
        &mut self.input
    }

    /// Shows `preview` below the hint, or no preview if it is `None`.
    pub fn set_preview(&mut self, preview: Option<Cow<'static, str>>) {
        self.preview = preview;
    }

    /// Returns the error shown below the preview, if any.
    #[must_use]
    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    /// Shows `error` below the preview in the `input_error` slot, or no error if it is `None`.
    pub fn set_error(&mut self, error: Option<Cow<'static, str>>) {
        self.error = error;
    }
}

/// Renders a [`Prompt`] as a popup centred in the given area.
///
/// The input is the first row inside the border; the hint, the preview and the error follow
/// in rows of their own, which stay empty while the line is absent so that the popup does not
/// change its size while the user types.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct InputPopup<'a> {
    prompt: &'a Prompt,
    theme: &'a Theme,
}

impl<'a> InputPopup<'a> {
    /// Creates a popup that shows `prompt`, styled by `theme`.
    #[must_use]
    pub const fn new(prompt: &'a Prompt, theme: &'a Theme) -> Self {
        Self { prompt, theme }
    }

    /// Renders the prompt centred in `area` and places the terminal cursor in its input.
    pub fn render_with_cursor(self, frame: &mut Frame, area: Rect) {
        let cursor_position = input_popup_inner(area)
            .rows()
            .next()
            .and_then(|row| self.prompt.input.cursor_position(row));
        frame.render_widget(self, area);
        if let Some(position) = cursor_position {
            frame.set_cursor_position(position);
        }
    }
}

impl Widget for InputPopup<'_> {
    fn render(self, area: Rect, buffer: &mut Buffer) {
        let prompt = self.prompt;
        let popup = centered_rect(area, INPUT_POPUP_HEIGHT);
        render_popup_block(&prompt.title, popup, buffer, self.theme);
        // Rows are taken from the top, so a popup cut short by a tiny terminal keeps its input.
        let mut rows = input_popup_inner(area).rows();
        let Some(input_row) = rows.next() else {
            return;
        };
        TextInputView::new(&prompt.input, "")
            .style(self.theme.style(Slot::Text))
            .render(input_row, buffer);
        let optional_lines = [
            (&prompt.hint, Slot::KeyHintLabel),
            (&prompt.preview, Slot::Text),
            (&prompt.error, Slot::InputError),
        ];
        for ((text, slot), row) in optional_lines.into_iter().zip(rows) {
            if let Some(text) = text {
                Line::styled(text.as_ref(), self.theme.style(slot)).render(row, buffer);
            }
        }
    }
}

/// Returns the area inside the border of an input popup centred in `area`: one row each for
/// the input, the hint, the preview and the error.
fn input_popup_inner(area: Rect) -> Rect {
    popup_block().inner(centered_rect(area, INPUT_POPUP_HEIGHT))
}

/// Asks the user to confirm `message`, showing the keys of `confirm-yes` and `confirm-no`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ConfirmDialog<'a> {
    message: &'a str,
    keymap: &'a Keymap,
    theme: &'a Theme,
}

impl<'a> ConfirmDialog<'a> {
    /// Creates a dialog asking `message`, with the keys from `keymap`, styled by `theme`.
    #[must_use]
    pub const fn new(message: &'a str, keymap: &'a Keymap, theme: &'a Theme) -> Self {
        Self {
            message,
            keymap,
            theme,
        }
    }
}

impl Widget for ConfirmDialog<'_> {
    fn render(self, area: Rect, buffer: &mut Buffer) {
        let message = Line::styled(self.message, self.theme.style(Slot::Text));
        let message_width = u16::try_from(message.width()).unwrap_or(u16::MAX);
        let popup = centered_rect_fitting(area, CONFIRM_DIALOG_HEIGHT, message_width);
        let inner = render_popup_block(CONFIRM_DIALOG_TITLE, popup, buffer, self.theme);
        // Rows are taken from the top, so a dialog cut short by a tiny terminal keeps its
        // message; the keys follow after an empty row.
        let mut rows = inner.rows();
        if let Some(message_row) = rows.next() {
            message.render(message_row, buffer);
        }
        if let Some(keys_row) = rows.nth(1) {
            confirm_keys(self.keymap, self.theme).render(keys_row, buffer);
        }
    }
}

/// Returns the keys of the [`CONFIRM_ACTIONS`] with their labels, such as `y/enter yes  n no`.
///
/// An action without keys is left out.
fn confirm_keys(keymap: &Keymap, theme: &Theme) -> Line<'static> {
    let mut spans = Vec::with_capacity(CONFIRM_ACTIONS.len() * 4);
    for action in CONFIRM_ACTIONS {
        let Some((first, others)) = keymap.keys(action).split_first() else {
            continue;
        };
        let mut keys = first.compact().to_string();
        for sequence in others {
            write!(keys, "{KEY_SEPARATOR}{}", sequence.compact())
                .expect("writing to a String cannot fail");
        }
        if !spans.is_empty() {
            spans.push(Span::raw("  "));
        }
        spans.extend([
            Span::styled(keys, theme.style(Slot::KeyHint)),
            Span::raw(" "),
            Span::styled(action.short_label(), theme.style(Slot::KeyHintLabel)),
        ]);
    }
    Line::from(spans)
}

/// The border shared by all popups, without title and styles.
pub(crate) fn popup_block() -> Block<'static> {
    Block::bordered()
        .border_type(BorderType::Rounded)
        .padding(Padding::horizontal(1))
}

/// Clears `popup`, draws the border titled `title` around it and returns the area inside.
pub(crate) fn render_popup_block(
    title: &str,
    popup: Rect,
    buffer: &mut Buffer,
    theme: &Theme,
) -> Rect {
    let block = popup_block()
        .style(theme.style(Slot::Text))
        .border_style(theme.style(Slot::PopupBorder))
        .title_style(theme.style(Slot::PopupTitle))
        .title(Line::from_iter([" ", title, " "]));
    let inner = block.inner(popup);
    Clear.render(popup, buffer);
    block.render(popup, buffer);
    inner
}

#[cfg(test)]
mod tests {
    use insta::assert_snapshot;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::layout::Position;
    use ratatui::style::Style;
    use rstest::rstest;

    use super::*;
    use crate::config::keymap::keymap::KeyBindings;

    /// Fills the screen before a popup is drawn, so that snapshots show what the popup clears.
    const BACKGROUND: &str = ".";
    const NORMAL_SIZE: (u16, u16) = (50, 10);
    const TINY_SIZE: (u16, u16) = (12, 3);

    fn keymap(yaml: &str) -> Keymap {
        let overrides: KeyBindings = if yaml.is_empty() {
            KeyBindings::default()
        } else {
            serde_saphyr::from_str(yaml).unwrap()
        };
        Keymap::new(&overrides).unwrap()
    }

    fn prompt() -> Prompt {
        Prompt::new("New task", TextInput::with_value("buy milk"))
    }

    fn prompt_with_all_lines() -> Prompt {
        let mut prompt = prompt().with_hint("enter save  esc cancel");
        prompt.set_preview(Some("due 2026-11-25".into()));
        prompt.set_error(Some("unknown date".into()));
        prompt
    }

    /// Fills a terminal of `size` with [`BACKGROUND`], draws `draw` over it and returns the
    /// terminal with its cursor.
    fn draw(size: (u16, u16), draw: impl FnOnce(&mut Frame)) -> Terminal<TestBackend> {
        let (width, height) = size;
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal
            .draw(|frame| {
                let area = frame.area();
                for position in area.positions() {
                    frame.buffer_mut()[position].set_symbol(BACKGROUND);
                }
                draw(frame);
            })
            .unwrap();
        terminal
    }

    fn draw_input_popup(size: (u16, u16), prompt: &Prompt) -> Terminal<TestBackend> {
        let theme = Theme::default();
        draw(size, |frame| {
            InputPopup::new(prompt, &theme).render_with_cursor(frame, frame.area());
        })
    }

    fn draw_confirm_dialog(size: (u16, u16), keymap: &Keymap) -> Terminal<TestBackend> {
        let theme = Theme::default();
        draw(size, |frame| {
            let dialog = ConfirmDialog::new("Delete list groceries?", keymap, &theme);
            frame.render_widget(dialog, frame.area());
        })
    }

    #[rstest]
    #[case::without_error(prompt(), None)]
    #[case::with_error(prompt_with_all_lines(), Some("unknown date"))]
    fn error_returns_the_shown_error(#[case] prompt: Prompt, #[case] expected: Option<&str>) {
        let system_under_test = prompt.error();

        assert_eq!(system_under_test, expected);
    }

    /// Returns the colour and modifiers of the cell at `x`, `y`; the background is never set.
    fn style_at(terminal: &Terminal<TestBackend>, x: u16, y: u16) -> Style {
        let cell = &terminal.backend().buffer()[(x, y)];
        Style::new().fg(cell.fg).add_modifier(cell.modifier)
    }

    #[rstest]
    #[case::wide(Rect::new(0, 0, 100, 30), 6, Rect::new(20, 12, 60, 6))]
    #[case::narrow_keeps_minimum_width(Rect::new(0, 0, 40, 20), 6, Rect::new(5, 7, 30, 6))]
    #[case::tiny_fits_the_screen(Rect::new(0, 0, 20, 4), 6, Rect::new(0, 0, 20, 4))]
    #[case::offset_area(Rect::new(10, 5, 50, 10), 4, Rect::new(20, 8, 30, 4))]
    #[case::empty_area(Rect::new(3, 2, 0, 0), 6, Rect::new(3, 2, 0, 0))]
    fn centered_rect_centres_a_sized_popup(
        #[case] area: Rect,
        #[case] height: u16,
        #[case] expected: Rect,
    ) {
        let system_under_test = centered_rect(area, height);

        assert_eq!(system_under_test, expected);
    }

    #[rstest]
    #[case::short_content_keeps_the_share(20, Rect::new(20, 12, 60, 6))]
    #[case::long_content_widens(70, Rect::new(13, 12, 74, 6))]
    #[case::too_long_content_fits_the_screen(200, Rect::new(0, 12, 100, 6))]
    fn centered_rect_fitting_widens_for_its_content(
        #[case] content_width: u16,
        #[case] expected: Rect,
    ) {
        let area = Rect::new(0, 0, 100, 30);

        let system_under_test = centered_rect_fitting(area, 6, content_width);

        assert_eq!(system_under_test, expected);
    }

    #[test]
    fn prompt_input_mut_edits_the_input() {
        let mut prompt = prompt();
        let expected = "buy milk!";

        let system_under_test = {
            prompt.input_mut().insert_char('!');
            prompt.input().value()
        };

        assert_eq!(system_under_test, expected);
    }

    #[test]
    fn prompt_set_preview_and_set_error_clear_lines_with_none() {
        let expected = prompt().with_hint("enter save  esc cancel");

        let system_under_test = {
            let mut prompt = prompt_with_all_lines();
            prompt.set_preview(None);
            prompt.set_error(None);
            prompt
        };

        assert_eq!(system_under_test, expected);
    }

    #[test]
    fn prompt_set_hint_replaces_the_hint() {
        let expected = prompt().with_hint("today, tomorrow");

        let system_under_test = {
            let mut prompt = prompt().with_hint("enter save");
            prompt.set_hint("today, tomorrow");
            prompt
        };

        assert_eq!(system_under_test, expected);
    }

    #[test]
    fn input_popup_without_optional_lines() {
        let terminal = draw_input_popup(NORMAL_SIZE, &prompt());

        let system_under_test = terminal.backend();

        assert_snapshot!(system_under_test);
    }

    #[test]
    fn input_popup_with_hint() {
        let terminal = draw_input_popup(NORMAL_SIZE, &prompt().with_hint("enter save"));

        let system_under_test = terminal.backend();

        assert_snapshot!(system_under_test);
    }

    #[test]
    fn input_popup_with_preview() {
        let mut prompt = prompt();
        prompt.set_preview(Some("due 2026-11-25".into()));
        let terminal = draw_input_popup(NORMAL_SIZE, &prompt);

        let system_under_test = terminal.backend();

        assert_snapshot!(system_under_test);
    }

    #[test]
    fn input_popup_with_error() {
        let mut prompt = prompt();
        prompt.set_error(Some("unknown date".into()));
        let terminal = draw_input_popup(NORMAL_SIZE, &prompt);

        let system_under_test = terminal.backend();

        assert_snapshot!(system_under_test);
    }

    #[test]
    fn input_popup_with_all_lines() {
        let terminal = draw_input_popup(NORMAL_SIZE, &prompt_with_all_lines());

        let system_under_test = terminal.backend();

        assert_snapshot!(system_under_test);
    }

    #[test]
    fn input_popup_on_tiny_terminal() {
        let terminal = draw_input_popup(TINY_SIZE, &prompt_with_all_lines());

        let system_under_test = terminal.backend();

        assert_snapshot!(system_under_test);
    }

    #[rstest]
    #[case::normal(NORMAL_SIZE, Position::new(20, 3))]
    #[case::tiny_scrolls_the_input(TINY_SIZE, Position::new(9, 1))]
    fn input_popup_places_the_cursor_in_its_input(
        #[case] size: (u16, u16),
        #[case] expected: Position,
    ) {
        let mut terminal = draw_input_popup(size, &prompt_with_all_lines());

        let system_under_test = terminal.get_cursor_position().unwrap();

        assert_eq!(system_under_test, expected);
    }

    #[rstest]
    #[case::one_cell((1, 1))]
    #[case::border_only((4, 2))]
    #[case::one_column((1, 8))]
    fn popups_render_on_minimal_terminals(#[case] size: (u16, u16)) {
        let keymap = keymap("");
        let theme = Theme::default();
        let expected = Rect::new(0, 0, size.0, size.1);

        let system_under_test = draw(size, |frame| {
            let prompt = prompt_with_all_lines();
            InputPopup::new(&prompt, &theme).render_with_cursor(frame, frame.area());
            let dialog = ConfirmDialog::new("Delete?", &keymap, &theme);
            frame.render_widget(dialog, frame.area());
        });

        assert_eq!(system_under_test.backend().buffer().area, expected);
    }

    #[test]
    fn input_popup_uses_the_popup_and_input_error_slots() {
        let theme = Theme::default();
        let expected = [
            theme.style(Slot::PopupBorder),
            theme.style(Slot::PopupTitle),
            theme.style(Slot::InputError),
        ];

        let system_under_test = draw_input_popup(NORMAL_SIZE, &prompt_with_all_lines());

        assert_eq!(style_at(&system_under_test, 10, 2), expected[0]);
        assert_eq!(style_at(&system_under_test, 12, 2), expected[1]);
        assert_eq!(style_at(&system_under_test, 12, 6), expected[2]);
    }

    #[test]
    fn confirm_dialog_with_default_keys() {
        let keymap = keymap("");
        let terminal = draw_confirm_dialog(NORMAL_SIZE, &keymap);

        let system_under_test = terminal.backend();

        assert_snapshot!(system_under_test);
    }

    #[test]
    fn confirm_dialog_with_rebound_and_unbound_keys() {
        let keymap = keymap("confirm-yes: ctrl-y\nconfirm-no: []");
        let terminal = draw_confirm_dialog(NORMAL_SIZE, &keymap);

        let system_under_test = terminal.backend();

        assert_snapshot!(system_under_test);
    }

    #[test]
    fn confirm_dialog_on_tiny_terminal() {
        let keymap = keymap("");
        let terminal = draw_confirm_dialog(TINY_SIZE, &keymap);

        let system_under_test = terminal.backend();

        assert_snapshot!(system_under_test);
    }

    #[test]
    fn confirm_dialog_uses_the_popup_and_key_hint_slots() {
        let theme = Theme::default();
        let keymap = keymap("");
        let expected = [
            theme.style(Slot::PopupBorder),
            theme.style(Slot::PopupTitle),
            theme.style(Slot::KeyHint),
            theme.style(Slot::KeyHintLabel),
        ];

        let system_under_test = draw_confirm_dialog(NORMAL_SIZE, &keymap);

        assert_eq!(style_at(&system_under_test, 10, 3), expected[0]);
        assert_eq!(style_at(&system_under_test, 12, 3), expected[1]);
        assert_eq!(style_at(&system_under_test, 12, 6), expected[2]);
        assert_eq!(style_at(&system_under_test, 20, 6), expected[3]);
    }
}

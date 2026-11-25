//! Single-line text editor used for task text, names, dates and search queries.
//!
//! [`TextInput`] holds the edited text and the cursor; it is changed by [`EditCommand`]s and by
//! inserting typed or pasted text. [`TextInputView`] renders it into one terminal row,
//! scrolling horizontally so that the cursor stays visible. The scroll offset is not stored:
//! it is computed from the text, the cursor and the width of the row on every render, so the
//! view only needs `&TextInput` and never changes the application state. The fixed editing keys are listed
//! once in [`EDITING_KEYS`], which both key handling and the help screen read.
//!
//! The cursor always rests on a grapheme cluster boundary, so combining sequences, emoji ZWJ
//! sequences and wide characters are moved over and deleted as a whole.

use std::borrow::Cow;
use std::ops::Range;

use ratatui::Frame;
use ratatui::buffer::{Buffer, CellWidth};
use ratatui::layout::{Position, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::widgets::Widget;
use unicode_segmentation::UnicodeSegmentation;

use crate::config::keymap::key::{Key, KeyChord, Modifiers};

/// Marks text that was cut off.
const ELLIPSIS: &str = "\u{2026}";

/// An editing operation on a [`TextInput`], triggered by one of the [`EDITING_KEYS`].
///
/// Words are runs of non-whitespace grapheme clusters, as in readline's `unix-word-rubout`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, strum::EnumIter)]
#[non_exhaustive]
pub enum EditCommand {
    /// Moves the cursor one grapheme cluster to the left.
    MoveLeft,
    /// Moves the cursor one grapheme cluster to the right.
    MoveRight,
    /// Moves the cursor to the start of the word before it.
    MoveWordLeft,
    /// Moves the cursor to the end of the word after it.
    MoveWordRight,
    /// Moves the cursor to the start of the text.
    MoveToStart,
    /// Moves the cursor to the end of the text.
    MoveToEnd,
    /// Deletes the grapheme cluster before the cursor.
    DeleteBackward,
    /// Deletes the grapheme cluster after the cursor.
    DeleteForward,
    /// Deletes the whitespace before the cursor, then the word before it.
    DeleteWordBackward,
    /// Deletes everything before the cursor.
    DeleteToStart,
    /// Deletes everything after the cursor.
    DeleteToEnd,
}

/// One row of the editing key table: the keys, what they do and the [`EditCommand`] they run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct EditingKey {
    keys: &'static [&'static str],
    description: &'static str,
    command: EditCommand,
}

impl EditingKey {
    /// Returns the keys in key notation (for example `ctrl-w`) that trigger the command.
    #[must_use]
    pub const fn keys(&self) -> &'static [&'static str] {
        self.keys
    }

    /// Returns the description shown on the help screen.
    #[must_use]
    pub const fn description(&self) -> &'static str {
        self.description
    }

    /// Returns the command the keys trigger.
    #[must_use]
    pub const fn command(&self) -> EditCommand {
        self.command
    }
}

/// The fixed editing keys of every text input, one row per [`EditCommand`].
///
/// Key handling maps these key notations onto commands, and the help screen lists them.
pub const EDITING_KEYS: [EditingKey; 11] = [
    EditingKey {
        keys: &["left"],
        description: "move left",
        command: EditCommand::MoveLeft,
    },
    EditingKey {
        keys: &["right"],
        description: "move right",
        command: EditCommand::MoveRight,
    },
    EditingKey {
        keys: &["ctrl-left"],
        description: "move to the previous word",
        command: EditCommand::MoveWordLeft,
    },
    EditingKey {
        keys: &["ctrl-right"],
        description: "move to the next word",
        command: EditCommand::MoveWordRight,
    },
    EditingKey {
        keys: &["home", "ctrl-a"],
        description: "move to the start",
        command: EditCommand::MoveToStart,
    },
    EditingKey {
        keys: &["end", "ctrl-e"],
        description: "move to the end",
        command: EditCommand::MoveToEnd,
    },
    EditingKey {
        keys: &["backspace"],
        description: "delete the character before the cursor",
        command: EditCommand::DeleteBackward,
    },
    EditingKey {
        keys: &["delete"],
        description: "delete the character under the cursor",
        command: EditCommand::DeleteForward,
    },
    EditingKey {
        keys: &["ctrl-w"],
        description: "delete the word before the cursor",
        command: EditCommand::DeleteWordBackward,
    },
    EditingKey {
        keys: &["ctrl-u"],
        description: "delete to the start",
        command: EditCommand::DeleteToStart,
    },
    EditingKey {
        keys: &["ctrl-k"],
        description: "delete to the end",
        command: EditCommand::DeleteToEnd,
    },
];

/// Tells whether a key press changed the text of a [`TextInput`].
#[must_use]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TextChange {
    /// Text was inserted or deleted.
    Changed,
    /// The text is the same; at most the cursor moved.
    Unchanged,
}

/// The state of a single-line text editor: its text and cursor.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
pub struct TextInput {
    value: String,
    /// Byte index into `value`, always on a grapheme cluster boundary.
    cursor: usize,
}

impl TextInput {
    /// Creates an empty text input.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Creates a text input holding `value` with the cursor at its end.
    ///
    /// Line breaks and other control characters become spaces, as with
    /// [`insert_str`](Self::insert_str).
    #[must_use]
    pub fn with_value(value: &str) -> Self {
        let mut input = Self::new();
        input.insert_str(value);
        input
    }

    /// Returns the current text.
    #[must_use]
    pub fn value(&self) -> &str {
        &self.value
    }

    /// Consumes the input and returns its text.
    #[must_use]
    pub fn into_value(self) -> String {
        self.value
    }

    /// Returns the cursor as a byte index into [`value`](Self::value), on a grapheme cluster
    /// boundary.
    #[must_use]
    pub const fn cursor(&self) -> usize {
        self.cursor
    }

    /// Applies an editing command.
    pub fn apply(&mut self, command: EditCommand) {
        match command {
            EditCommand::MoveLeft => self.cursor = self.previous_boundary(),
            EditCommand::MoveRight => self.cursor = self.next_boundary(),
            EditCommand::MoveWordLeft => self.cursor = self.previous_word_start(),
            EditCommand::MoveWordRight => self.cursor = self.next_word_end(),
            EditCommand::MoveToStart => self.cursor = 0,
            EditCommand::MoveToEnd => self.cursor = self.value.len(),
            EditCommand::DeleteBackward => self.delete(self.previous_boundary()..self.cursor),
            EditCommand::DeleteForward => self.delete(self.cursor..self.next_boundary()),
            EditCommand::DeleteWordBackward => {
                self.delete(self.previous_word_start()..self.cursor);
            }
            EditCommand::DeleteToStart => self.delete(0..self.cursor),
            EditCommand::DeleteToEnd => self.delete(self.cursor..self.value.len()),
        }
    }

    /// Inserts a typed character at the cursor and moves the cursor behind it.
    ///
    /// A control character is inserted as a space.
    pub fn insert_char(&mut self, character: char) {
        self.insert_str(character.encode_utf8(&mut [0; 4]));
    }

    /// Inserts pasted text at the cursor and moves the cursor behind it.
    ///
    /// Every line break (`\n`, `\r\n` or `\r`) and every other control character becomes a
    /// space, because the input holds a single line.
    pub fn insert_str(&mut self, text: &str) {
        let text = to_single_line(text);
        self.replace(self.cursor..self.cursor, &text);
    }

    /// Handles a key press that no action is bound to: one of the [`EDITING_KEYS`] applies its
    /// command, a character without modifiers is inserted, and every other chord is ignored.
    pub fn press(&mut self, chord: KeyChord) -> TextChange {
        let length_before = self.value.len();
        if let Some(command) = edit_command(chord) {
            self.apply(command);
        } else if let Key::Char(character) = chord.key()
            && chord.modifiers() == Modifiers::NONE
        {
            self.insert_char(character);
        }
        // Every edit that changes the text inserts or deletes at least one byte.
        if self.value.len() == length_before {
            TextChange::Unchanged
        } else {
            TextChange::Changed
        }
    }

    /// Returns the terminal position of the cursor when the input is rendered into `area` by
    /// [`TextInputView`], or `None` if the cursor is not inside `area`.
    #[must_use]
    pub fn cursor_position(&self, area: Rect) -> Option<Position> {
        let column = self.cursor_column().checked_sub(self.scroll(area.width))?;
        let column = u16::try_from(column)
            .ok()
            .filter(|&column| column < area.width)?;
        (area.height > 0).then(|| Position::new(area.x + column, area.y))
    }

    fn delete(&mut self, range: Range<usize>) {
        self.replace(range, "");
    }

    /// Replaces `range` with `replacement` and puts the cursor behind the replacement.
    ///
    /// The edit can merge grapheme clusters (inserting a zero width joiner between two emoji,
    /// for example), so the cursor moves forward to the next boundary if necessary.
    fn replace(&mut self, range: Range<usize>, replacement: &str) {
        let end = range.start + replacement.len();
        self.value.replace_range(range, replacement);
        self.cursor = self.boundary_at_or_after(end);
    }

    fn boundary_at_or_after(&self, index: usize) -> usize {
        self.value
            .grapheme_indices(true)
            .map(|(boundary, _)| boundary)
            .find(|&boundary| boundary >= index)
            .unwrap_or(self.value.len())
    }

    fn previous_boundary(&self) -> usize {
        self.value[..self.cursor]
            .grapheme_indices(true)
            .next_back()
            .map_or(0, |(boundary, _)| boundary)
    }

    fn next_boundary(&self) -> usize {
        self.value[self.cursor..]
            .graphemes(true)
            .next()
            .map_or(self.cursor, |grapheme| self.cursor + grapheme.len())
    }

    fn previous_word_start(&self) -> usize {
        self.value[..self.cursor]
            .grapheme_indices(true)
            .rev()
            .skip_while(|(_, grapheme)| is_whitespace(grapheme))
            .take_while(|(_, grapheme)| !is_whitespace(grapheme))
            .last()
            .map_or(0, |(boundary, _)| boundary)
    }

    fn next_word_end(&self) -> usize {
        self.value[self.cursor..]
            .grapheme_indices(true)
            .skip_while(|(_, grapheme)| is_whitespace(grapheme))
            .find(|(_, grapheme)| is_whitespace(grapheme))
            .map_or(self.value.len(), |(offset, _)| self.cursor + offset)
    }

    fn cursor_column(&self) -> usize {
        display_width(&self.value[..self.cursor])
    }

    /// Returns the display column shown in the leftmost cell of a row `width` cells wide: the
    /// text scrolls just far enough to show the cursor cell, so the cursor rests at the right
    /// edge while it is right of the first `width` columns.
    fn scroll(&self, width: u16) -> usize {
        (self.cursor_column() + 1).saturating_sub(usize::from(width))
    }

    /// Returns the byte index and display column of the first grapheme cluster that starts at
    /// or after the display column `scroll`.
    fn first_visible_grapheme(&self, scroll: usize) -> (usize, usize) {
        let mut column = 0;
        for (index, grapheme) in self.value.grapheme_indices(true) {
            if column >= scroll {
                return (index, column);
            }
            column += usize::from(grapheme.cell_width());
        }
        (self.value.len(), column)
    }
}

/// Renders a [`TextInput`] into the first row of an area.
///
/// The text scrolls horizontally so that the cursor stays visible. An empty input shows a
/// placeholder, dimmed by default.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct TextInputView<'a> {
    input: &'a TextInput,
    placeholder: &'a str,
    style: Style,
    placeholder_style: Style,
}

impl<'a> TextInputView<'a> {
    /// Creates a view of `input` that shows `placeholder` while the input is empty.
    #[must_use]
    pub const fn new(input: &'a TextInput, placeholder: &'a str) -> Self {
        Self {
            input,
            placeholder,
            style: Style::new(),
            placeholder_style: Style::new().add_modifier(Modifier::DIM),
        }
    }

    /// Sets the style of the whole row and the text.
    #[must_use]
    pub const fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }

    /// Sets the style of the placeholder, patched onto the row style.
    #[must_use]
    pub const fn placeholder_style(mut self, style: Style) -> Self {
        self.placeholder_style = style;
        self
    }

    /// Renders the input into `area` and places the terminal cursor at the input's cursor.
    pub fn render_with_cursor(self, frame: &mut Frame, area: Rect) {
        let cursor_position = self.input.cursor_position(area);
        frame.render_widget(self, area);
        if let Some(position) = cursor_position {
            frame.set_cursor_position(position);
        }
    }
}

impl Widget for TextInputView<'_> {
    fn render(self, area: Rect, buffer: &mut Buffer) {
        let input = self.input;
        let area = area.intersection(buffer.area);
        if area.is_empty() {
            return;
        }
        buffer.set_style(area, self.style);
        let max_width = usize::from(area.width);
        if input.value.is_empty() {
            let style = self.style.patch(self.placeholder_style);
            buffer.set_stringn(area.x, area.y, self.placeholder, max_width, style);
            return;
        }
        let scroll = input.scroll(area.width);
        let (start, column) = input.first_visible_grapheme(scroll);
        // A wide grapheme cut by the left edge leaves its visible cells empty.
        let padding = column - scroll;
        let Some(x) = u16::try_from(padding)
            .ok()
            .and_then(|padding| area.x.checked_add(padding))
        else {
            return;
        };
        let visible_text = &input.value[start..];
        buffer.set_stringn(
            x,
            area.y,
            visible_text,
            max_width.saturating_sub(padding),
            self.style,
        );
    }
}

/// Returns the command of the [`EDITING_KEYS`] row that lists `chord`, if any.
fn edit_command(chord: KeyChord) -> Option<EditCommand> {
    EDITING_KEYS
        .iter()
        .find(|editing_key| {
            editing_key
                .keys
                .iter()
                .any(|notation| notation.parse::<KeyChord>().is_ok_and(|key| key == chord))
        })
        .map(EditingKey::command)
}

/// Replaces line breaks and other control characters with spaces; `\r\n` becomes one space.
fn to_single_line(text: &str) -> Cow<'_, str> {
    if !text.contains(char::is_control) {
        return Cow::Borrowed(text);
    }
    let mut single_line = String::with_capacity(text.len());
    let mut characters = text.chars().peekable();
    while let Some(character) = characters.next() {
        if character == '\r' {
            characters.next_if_eq(&'\n');
        }
        single_line.push(if character.is_control() {
            ' '
        } else {
            character
        });
    }
    Cow::Owned(single_line)
}

fn is_whitespace(grapheme: &str) -> bool {
    grapheme.chars().all(char::is_whitespace)
}

/// Returns the number of terminal cells `text` occupies, measured exactly as [`Buffer`]
/// measures it while rendering.
pub(crate) fn display_width(text: &str) -> usize {
    text.graphemes(true)
        .map(|grapheme| usize::from(grapheme.cell_width()))
        .sum()
}

/// Returns `text` cut to at most `width` cells, ending in an ellipsis if it was cut.
pub(crate) fn truncated(text: &str, width: usize) -> Cow<'_, str> {
    match fitted(text, width) {
        (kept, "") => Cow::Borrowed(kept),
        (kept, ellipsis) => Cow::Owned(format!("{kept}{ellipsis}")),
    }
}

/// Splits [`truncated`] into the longest prefix of `text` that is shown in `width` cells and
/// the ellipsis that follows it, which is empty if `text` fits or no cell is left for it.
///
/// Callers that style parts of `text` style the prefix and append the ellipsis.
pub(crate) fn fitted(text: &str, width: usize) -> (&str, &'static str) {
    if display_width(text) <= width {
        return (text, "");
    }
    let Some(text_width) = width.checked_sub(display_width(ELLIPSIS)) else {
        return ("", "");
    };
    let mut used_width = 0;
    let end = text
        .grapheme_indices(true)
        .take_while(|(_, grapheme)| {
            used_width += usize::from(grapheme.cell_width());
            used_width <= text_width
        })
        .last()
        .map_or(0, |(start, grapheme)| start + grapheme.len());
    (&text[..end], ELLIPSIS)
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use rstest::rstest;
    use strum::IntoEnumIterator;

    use super::*;
    use crate::user_docs::KEYBINDINGS;

    /// Marks the cursor position in the text of test cases.
    const CURSOR_MARK: char = '|';
    const WOMAN: &str = "\u{1F469}";
    const GIRL: &str = "\u{1F467}";
    const FAMILY: &str = "\u{1F469}\u{200D}\u{1F469}\u{200D}\u{1F467}";

    /// Builds an input from text containing [`CURSOR_MARK`] at the cursor position.
    fn input(marked: &str) -> TextInput {
        let cursor = marked
            .find(CURSOR_MARK)
            .expect("test text contains the cursor mark");
        TextInput {
            value: marked.replacen(CURSOR_MARK, "", 1),
            cursor,
        }
    }

    /// Returns the text of `input` with [`CURSOR_MARK`] at the cursor position.
    fn marked(input: &TextInput) -> String {
        let (before, after) = input.value.split_at(input.cursor);
        format!("{before}{CURSOR_MARK}{after}")
    }

    /// Applies `operation` to the input described by `before` and returns the marked result.
    fn edit(before: &str, operation: impl FnOnce(&mut TextInput)) -> String {
        let mut text_input = input(before);
        operation(&mut text_input);
        marked(&text_input)
    }

    fn render_buffer(view: TextInputView<'_>, area: Rect) -> Buffer {
        let mut buffer = Buffer::empty(area);
        view.render(area, &mut buffer);
        buffer
    }

    fn render(input: &TextInput, width: u16) -> (Buffer, Position) {
        let mut terminal = Terminal::new(TestBackend::new(width, 1)).unwrap();
        terminal
            .draw(|frame| {
                TextInputView::new(input, "Search").render_with_cursor(frame, frame.area());
            })
            .unwrap();
        let cursor = terminal.get_cursor_position().unwrap();
        (terminal.backend().buffer().clone(), cursor)
    }

    #[rstest]
    #[case::left("ab|", EditCommand::MoveLeft, "a|b")]
    #[case::left_at_start("|ab", EditCommand::MoveLeft, "|ab")]
    #[case::left_over_combining("ae\u{301}|", EditCommand::MoveLeft, "a|e\u{301}")]
    #[case::left_over_zwj_sequence(&format!("a{FAMILY}|"), EditCommand::MoveLeft, &format!("a|{FAMILY}"))]
    #[case::right("a|b", EditCommand::MoveRight, "ab|")]
    #[case::right_at_end("ab|", EditCommand::MoveRight, "ab|")]
    #[case::right_over_combining("|e\u{301}a", EditCommand::MoveRight, "e\u{301}|a")]
    #[case::right_over_zwj_sequence(&format!("|{FAMILY}a"), EditCommand::MoveRight, &format!("{FAMILY}|a"))]
    #[case::right_over_wide("|\u{65E5}\u{672C}", EditCommand::MoveRight, "\u{65E5}|\u{672C}")]
    #[case::word_left_from_word_end("foo bar|", EditCommand::MoveWordLeft, "foo |bar")]
    #[case::word_left_from_inside_word("foo ba|r", EditCommand::MoveWordLeft, "foo |bar")]
    #[case::word_left_over_whitespace("foo bar  |", EditCommand::MoveWordLeft, "foo |bar  ")]
    #[case::word_left_to_start("  foo|", EditCommand::MoveWordLeft, "  |foo")]
    #[case::word_left_at_start("|  foo", EditCommand::MoveWordLeft, "|  foo")]
    #[case::word_right_from_word_start("|foo bar", EditCommand::MoveWordRight, "foo| bar")]
    #[case::word_right_over_whitespace("foo| bar", EditCommand::MoveWordRight, "foo bar|")]
    #[case::word_right_at_end("foo|", EditCommand::MoveWordRight, "foo|")]
    #[case::to_start("ab|c", EditCommand::MoveToStart, "|abc")]
    #[case::to_end("a|bc", EditCommand::MoveToEnd, "abc|")]
    #[case::backspace("ab|c", EditCommand::DeleteBackward, "a|c")]
    #[case::backspace_at_start("|ab", EditCommand::DeleteBackward, "|ab")]
    #[case::backspace_combining("ae\u{301}|", EditCommand::DeleteBackward, "a|")]
    #[case::backspace_zwj_sequence(&format!("a{FAMILY}|"), EditCommand::DeleteBackward, "a|")]
    #[case::delete("a|bc", EditCommand::DeleteForward, "a|c")]
    #[case::delete_at_end("ab|", EditCommand::DeleteForward, "ab|")]
    #[case::delete_zwj_sequence(&format!("a|{FAMILY}b"), EditCommand::DeleteForward, "a|b")]
    #[case::delete_wide("|\u{65E5}\u{672C}", EditCommand::DeleteForward, "|\u{672C}")]
    #[case::delete_word("foo bar|", EditCommand::DeleteWordBackward, "foo |")]
    #[case::delete_word_whitespace_first("foo bar  |", EditCommand::DeleteWordBackward, "foo |")]
    #[case::delete_word_inside("foo ba|r", EditCommand::DeleteWordBackward, "foo |r")]
    #[case::delete_word_wide(
        "\u{65E5} \u{672C}\u{8A9E}|",
        EditCommand::DeleteWordBackward,
        "\u{65E5} |"
    )]
    #[case::delete_word_at_start("|foo", EditCommand::DeleteWordBackward, "|foo")]
    #[case::delete_to_start("foo b|ar", EditCommand::DeleteToStart, "|ar")]
    #[case::delete_to_end("foo b|ar", EditCommand::DeleteToEnd, "foo b|")]
    fn apply_edits_text_and_cursor(
        #[case] before: &str,
        #[case] command: EditCommand,
        #[case] expected: &str,
    ) {
        let system_under_test = edit(before, |text_input| text_input.apply(command));

        assert_eq!(system_under_test, expected);
    }

    #[rstest]
    #[case::ascii("a|c", "b", "ab|c")]
    #[case::combining_mark("e|x", "\u{301}", "e\u{301}|x")]
    #[case::joiner_merges_clusters(&format!("{WOMAN}|{GIRL}"), "\u{200D}", &format!("{WOMAN}\u{200D}{GIRL}|"))]
    #[case::wide("|", "\u{65E5}", "\u{65E5}|")]
    #[case::paste_with_line_breaks("|!", "one\ntwo\r\nthree\r", "one two three |!")]
    #[case::paste_with_tab("|", "a\tb", "a b|")]
    fn insert_str_inserts_single_line_text(
        #[case] before: &str,
        #[case] text: &str,
        #[case] expected: &str,
    ) {
        let system_under_test = edit(before, |text_input| text_input.insert_str(text));

        assert_eq!(system_under_test, expected);
    }

    #[rstest]
    #[case::printable('x', "ax|")]
    #[case::control('\n', "a |")]
    fn insert_char_inserts_at_cursor(#[case] character: char, #[case] expected: &str) {
        let system_under_test = edit("a|", |text_input| text_input.insert_char(character));

        assert_eq!(system_under_test, expected);
    }

    #[rstest]
    #[case::character("ab|", "c", "abc|", TextChange::Changed)]
    #[case::shifted_character("ab|", "?", "ab?|", TextChange::Changed)]
    #[case::space("ab|", "space", "ab |", TextChange::Changed)]
    #[case::editing_key_moves("ab|", "ctrl-a", "|ab", TextChange::Unchanged)]
    #[case::editing_key_with_second_notation("|ab", "end", "ab|", TextChange::Unchanged)]
    #[case::editing_key_deletes("foo bar|", "ctrl-w", "foo |", TextChange::Changed)]
    #[case::deleting_nothing("|ab", "backspace", "|ab", TextChange::Unchanged)]
    #[case::character_with_modifier("ab|", "alt-c", "ab|", TextChange::Unchanged)]
    #[case::other_key("ab|", "f5", "ab|", TextChange::Unchanged)]
    fn press_edits_or_inserts(
        #[case] before: &str,
        #[case] notation: &str,
        #[case] expected_text: &str,
        #[case] expected_change: TextChange,
    ) {
        let chord: KeyChord = notation.parse().unwrap();
        let mut text_input = input(before);

        let system_under_test = text_input.press(chord);

        assert_eq!(system_under_test, expected_change);
        assert_eq!(marked(&text_input), expected_text);
    }

    #[test]
    fn every_editing_key_notation_runs_its_command() {
        let expected: Vec<Option<EditCommand>> = EDITING_KEYS
            .iter()
            .flat_map(|editing_key| {
                editing_key
                    .keys()
                    .iter()
                    .map(|_| Some(editing_key.command()))
            })
            .collect();

        let system_under_test: Vec<Option<EditCommand>> = EDITING_KEYS
            .iter()
            .flat_map(EditingKey::keys)
            .map(|notation| edit_command(notation.parse().unwrap()))
            .collect();

        assert_eq!(system_under_test, expected);
    }

    #[test]
    fn with_value_puts_cursor_at_end_and_joins_lines() {
        let value = "buy\nmilk";
        let expected = "buy milk|";

        let system_under_test = TextInput::with_value(value);

        assert_eq!(marked(&system_under_test), expected);
    }

    #[test]
    fn into_value_returns_text() {
        let text_input = TextInput::with_value("milk");
        let expected = "milk";

        let system_under_test = text_input.into_value();

        assert_eq!(system_under_test, expected);
    }

    #[test]
    fn every_edit_command_has_exactly_one_row_in_editing_keys() {
        let expected: Vec<(EditCommand, usize)> =
            EditCommand::iter().map(|command| (command, 1)).collect();

        let system_under_test: Vec<(EditCommand, usize)> = EditCommand::iter()
            .map(|command| {
                let rows = EDITING_KEYS.iter().filter(|key| key.command() == command);
                (command, rows.count())
            })
            .collect();

        assert_eq!(system_under_test, expected);
    }

    #[test]
    fn editing_keys_are_unique_and_described() {
        let all_keys: Vec<&str> = EDITING_KEYS
            .iter()
            .flat_map(EditingKey::keys)
            .copied()
            .collect();
        let expected_count = all_keys.len();

        let system_under_test: HashSet<&str> = all_keys.into_iter().collect();

        assert_eq!(system_under_test.len(), expected_count);
        assert!(EDITING_KEYS.iter().all(|key| !key.description().is_empty()));
    }

    #[test]
    fn keybinding_documentation_lists_every_editing_key() {
        let expected: Vec<&str> = Vec::new();

        let system_under_test: Vec<&str> = EDITING_KEYS
            .iter()
            .flat_map(EditingKey::keys)
            .copied()
            .filter(|key| !KEYBINDINGS.text().contains(&format!("`{key}`")))
            .collect();

        assert_eq!(system_under_test, expected);
    }

    #[rstest]
    #[case::fits("hel|lo", 8, "hello   ", 3)]
    #[case::cursor_at_end_scrolls("abcdefghij|", 5, "ghij ", 4)]
    #[case::combining_marks("e\u{301}e\u{301}|", 4, "e\u{301}e\u{301}  ", 2)]
    #[case::zwj_sequence(&format!("{FAMILY}|a"), 4, &format!("{FAMILY}a "), 2)]
    #[case::wide_cut_at_right_edge("|abc\u{65E5}", 4, "abc ", 0)]
    #[case::wide_cut_at_left_edge("\u{65E5}\u{672C}a|", 3, " a ", 2)]
    #[case::wide_at_cursor_scrolls("a\u{65E5}\u{672C}|", 3, "\u{672C} ", 2)]
    #[case::cursor_at_start_shows_the_start("|abcdefghij", 5, "abcde", 0)]
    #[case::cursor_inside_the_first_columns("ab|cdefghij", 5, "abcde", 2)]
    #[case::cursor_further_right_rests_at_the_right_edge("abcdefgh|ij", 5, "efghi", 4)]
    fn render_scrolls_to_cursor(
        #[case] text: &str,
        #[case] width: u16,
        #[case] expected_line: &str,
        #[case] expected_cursor_x: u16,
    ) {
        let text_input = input(text);
        let expected_buffer = Buffer::with_lines([expected_line]);
        let expected_cursor = Position::new(expected_cursor_x, 0);

        let system_under_test = render(&text_input, width);

        assert_eq!(system_under_test, (expected_buffer, expected_cursor));
    }

    #[test]
    fn render_shows_dimmed_placeholder_when_empty() {
        let text_input = TextInput::new();
        let mut expected_buffer = Buffer::with_lines(["Search  "]);
        expected_buffer.set_style(
            Rect::new(0, 0, 6, 1),
            Style::new().add_modifier(Modifier::DIM),
        );
        let expected_cursor = Position::new(0, 0);

        let system_under_test = render(&text_input, 8);

        assert_eq!(system_under_test, (expected_buffer, expected_cursor));
    }

    #[test]
    fn render_applies_style_to_whole_row() {
        let text_input = input("ab|");
        let style = Style::new().add_modifier(Modifier::BOLD);
        let mut expected = Buffer::with_lines(["ab  "]);
        expected.set_style(Rect::new(0, 0, 4, 1), style);
        let view = TextInputView::new(&text_input, "").style(style);

        let system_under_test = render_buffer(view, Rect::new(0, 0, 4, 1));

        assert_eq!(system_under_test, expected);
    }

    #[test]
    fn render_into_empty_area_draws_nothing() {
        let text_input = input("ab|");
        let area = Rect::new(0, 0, 0, 1);
        let expected = Buffer::empty(area);

        let system_under_test = render_buffer(TextInputView::new(&text_input, ""), area);

        assert_eq!(system_under_test, expected);
    }

    #[rstest]
    #[case::zero_height(Rect::new(0, 0, 5, 0))]
    #[case::zero_width(Rect::new(0, 0, 0, 1))]
    fn cursor_position_is_none_outside_area(#[case] area: Rect) {
        let text_input = input("ab|");

        let system_under_test = text_input.cursor_position(area);

        assert_eq!(system_under_test, None);
    }

    #[rstest]
    #[case::fits("close", 5, "close")]
    #[case::cut("close the help", 8, "close t\u{2026}")]
    #[case::only_the_ellipsis("close", 1, "\u{2026}")]
    #[case::nothing("close", 0, "")]
    #[case::wide_character_does_not_overflow("\u{65E5}\u{672C}\u{8A9E}", 4, "\u{65E5}\u{2026}")]
    #[case::combining_mark_stays_whole("e\u{301}e\u{301}e\u{301}", 2, "e\u{301}\u{2026}")]
    fn truncated_cuts_with_an_ellipsis(
        #[case] text: &str,
        #[case] width: usize,
        #[case] expected: &str,
    ) {
        let system_under_test = truncated(text, width);

        assert_eq!(system_under_test, expected);
    }

    #[rstest]
    #[case::fits("close", 5, ("close", ""))]
    #[case::cut("close the help", 8, ("close t", "\u{2026}"))]
    #[case::only_the_ellipsis("close", 1, ("", "\u{2026}"))]
    #[case::nothing("close", 0, ("", ""))]
    fn fitted_splits_the_kept_prefix_from_the_ellipsis(
        #[case] text: &str,
        #[case] width: usize,
        #[case] expected: (&str, &str),
    ) {
        let system_under_test = fitted(text, width);

        assert_eq!(system_under_test, expected);
    }
}

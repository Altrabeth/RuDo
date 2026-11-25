//! The popup of a fuzzy [`Picker`]: the typed text on top, the matching candidates below it
//! with their matched characters highlighted, and a hint or an error in the last row.
//!
//! The popup depends only on the picker, its title and message and the theme, never on what
//! the candidates are. It draws only the visible results and scrolls without stored state:
//! the first visible result is derived from the selection and the height on every render,
//! just far enough to show the selection, like the horizontal scrolling of
//! [`TextInputView`]. The bottom border shows which results are visible, as `top-bottom/total`.

use ratatui::Frame;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::text::Line;
use ratatui::widgets::Widget;

use crate::app::picker::{Picker, PickerCandidate, PickerResult};
use crate::config::theme::{Slot, Theme};
use crate::ui::help_overlay::position_indicator;
use crate::ui::highlight::push_highlighted_spans;
use crate::ui::popup::{centered_rect, popup_block, render_popup_block};
use crate::ui::text_input::TextInputView;

/// Two border rows, the input, ten results and the message.
const PICKER_POPUP_HEIGHT: u16 = 14;
/// Shown instead of the results when nothing matches the typed text.
const NO_MATCHES: &str = "no matches";

/// Renders a [`Picker`] as a popup centred in the given area.
#[derive(Debug, Clone, Copy)]
pub struct PickerPopup<'a, C> {
    picker: &'a Picker<C>,
    title: &'a str,
    hint: Option<&'a str>,
    error: Option<&'a str>,
    theme: &'a Theme,
}

impl<'a, C: PickerCandidate> PickerPopup<'a, C> {
    /// Creates a popup titled `title` that shows `picker`, styled by `theme`.
    #[must_use]
    pub const fn new(picker: &'a Picker<C>, title: &'a str, theme: &'a Theme) -> Self {
        Self {
            picker,
            title,
            hint: None,
            error: None,
            theme,
        }
    }

    /// Shows `hint` below the results, unless there is an error.
    #[must_use]
    pub const fn hint(mut self, hint: Option<&'a str>) -> Self {
        self.hint = hint;
        self
    }

    /// Shows `error` below the results in the `input_error` slot, instead of the hint.
    #[must_use]
    pub const fn error(mut self, error: Option<&'a str>) -> Self {
        self.error = error;
        self
    }

    /// Renders the picker centred in `area` and places the terminal cursor in its input.
    pub fn render_with_cursor(self, frame: &mut Frame, area: Rect) {
        let input_row = PickerLayout::new(inner_area(area)).input;
        let cursor_position = self.picker.input().cursor_position(input_row);
        frame.render_widget(self, area);
        if let Some(position) = cursor_position {
            frame.set_cursor_position(position);
        }
    }

    /// Draws the visible results into `area`, starting at result `offset`, and the selected
    /// one in the `selection` slot; or says that nothing matches.
    fn render_results(&self, area: Rect, offset: usize, buffer: &mut Buffer) {
        if self.picker.result_count() == 0 {
            if let Some(row) = area.rows().next() {
                Line::styled(NO_MATCHES, self.theme.style(Slot::KeyHintLabel)).render(row, buffer);
            }
            return;
        }
        let selected = self.picker.selected_index();
        let rows = area.rows().zip(offset..);
        for ((row, index), result) in rows.zip(self.picker.results(offset)) {
            self.result_line(result).render(row, buffer);
            if selected == Some(index) {
                buffer.set_style(row, self.theme.style(Slot::Selection));
            }
        }
    }

    /// Returns the text of `result` with its matched characters in the `search_match` slot.
    fn result_line(&self, result: PickerResult<'a, C>) -> Line<'a> {
        let mut spans = Vec::with_capacity(2 * result.ranges().len() + 1);
        push_highlighted_spans(
            &mut spans,
            result.candidate().text(),
            0,
            self.theme.style(Slot::Text),
            result.ranges(),
            self.theme.style(Slot::SearchMatch),
        );
        Line::from(spans)
    }

    /// Draws the error below the results, or else the hint, if any.
    fn render_message(&self, row: Rect, buffer: &mut Buffer) {
        let message = match (self.error, self.hint) {
            (Some(error), _) => Some((error, Slot::InputError)),
            (None, Some(hint)) => Some((hint, Slot::KeyHintLabel)),
            (None, None) => None,
        };
        if let Some((text, slot)) = message {
            Line::styled(text, self.theme.style(slot)).render(row, buffer);
        }
    }
}

impl<C: PickerCandidate> Widget for PickerPopup<'_, C> {
    fn render(self, area: Rect, buffer: &mut Buffer) {
        let popup = centered_rect(area, PICKER_POPUP_HEIGHT);
        let inner = render_popup_block(self.title, popup, buffer, self.theme);
        let layout = PickerLayout::new(inner);
        TextInputView::new(self.picker.input(), "")
            .style(self.theme.style(Slot::Text))
            .render(layout.input, buffer);
        let visible = usize::from(layout.results.height);
        let offset = first_visible(self.picker.selected_index(), visible);
        self.render_results(layout.results, offset, buffer);
        if let Some(row) = layout.message {
            self.render_message(row, buffer);
        }
        let total = self.picker.result_count();
        if total > 0 && visible > 0 && popup.height > 1 {
            let bottom_border = Rect::new(
                popup.x + 1,
                popup.bottom() - 1,
                popup.width.saturating_sub(2),
                1,
            );
            Line::styled(
                position_indicator(offset, visible, total),
                self.theme.style(Slot::PopupTitle),
            )
            .right_aligned()
            .render(bottom_border, buffer);
        }
    }
}

/// Returns the area inside the border of a picker popup centred in `area`.
fn inner_area(area: Rect) -> Rect {
    popup_block().inner(centered_rect(area, PICKER_POPUP_HEIGHT))
}

/// The rows inside the border of a picker popup.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct PickerLayout {
    input: Rect,
    results: Rect,
    /// The last row, if the popup is high enough to show a result as well.
    message: Option<Rect>,
}

impl PickerLayout {
    /// Divides `inner` from the top, so that a popup cut short by a tiny terminal keeps its
    /// input and then its results.
    fn new(inner: Rect) -> Self {
        let input = Rect {
            height: inner.height.min(1),
            ..inner
        };
        let below = inner.height.saturating_sub(1);
        let message = (below >= 2).then(|| Rect {
            y: inner.bottom() - 1,
            height: 1,
            ..inner
        });
        let results = Rect {
            y: inner.y + input.height,
            height: below - u16::from(message.is_some()),
            ..inner
        };
        Self {
            input,
            results,
            message,
        }
    }
}

/// Returns the first result to show in `visible` rows so that the `selected` result is
/// visible: the top of the list until the selection passes the last row.
fn first_visible(selected: Option<usize>, visible: usize) -> usize {
    selected.map_or(0, |selected| (selected + 1).saturating_sub(visible))
}

#[cfg(test)]
mod tests {
    use insta::assert_snapshot;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::layout::Position;
    use ratatui::style::{Color, Style};
    use rstest::rstest;

    use super::*;

    /// Fills the screen before the popup is drawn, so that snapshots show what it clears.
    const BACKGROUND: &str = ".";
    const NORMAL_SIZE: (u16, u16) = (50, 18);

    /// A candidate whose name is its last path component.
    #[derive(Debug)]
    struct Folder(String);

    impl PickerCandidate for Folder {
        fn text(&self) -> &str {
            &self.0
        }

        fn name_start(&self) -> usize {
            self.0.rfind('/').map_or(0, |slash| slash + 1)
        }
    }

    fn picker(folders: &[&str], typed: &str) -> Picker<Folder> {
        let mut picker = Picker::new(
            folders
                .iter()
                .map(|folder| Folder((*folder).to_owned()))
                .collect(),
        );
        if !typed.is_empty() {
            picker.paste(typed);
        }
        picker
    }

    fn many_folders(count: usize) -> Picker<Folder> {
        let folders: Vec<String> = (1..=count)
            .map(|number| format!("Folder {number}"))
            .collect();
        let names: Vec<&str> = folders.iter().map(String::as_str).collect();
        picker(&names, "")
    }

    fn draw(size: (u16, u16), popup: PickerPopup<'_, Folder>) -> Terminal<TestBackend> {
        let (width, height) = size;
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal
            .draw(|frame| {
                let area = frame.area();
                for position in area.positions() {
                    frame.buffer_mut()[position].set_symbol(BACKGROUND);
                }
                popup.render_with_cursor(frame, area);
            })
            .unwrap();
        terminal
    }

    fn style_at(terminal: &Terminal<TestBackend>, x: u16, y: u16) -> Style {
        let cell = &terminal.backend().buffer()[(x, y)];
        Style::new().fg(cell.fg).add_modifier(cell.modifier)
    }

    #[test]
    fn popup_lists_the_matches_with_the_count() {
        let picker = picker(&["/", "Home", "Home/Garden", "Work"], "o");
        let theme = Theme::default();
        let terminal = draw(
            NORMAL_SIZE,
            PickerPopup::new(&picker, "Move to folder", &theme),
        );

        let system_under_test = terminal.backend();

        assert_snapshot!(system_under_test);
    }

    #[test]
    fn popup_without_matches_says_so_and_shows_the_error() {
        let picker = picker(&["/", "Home"], "xyz");
        let theme = Theme::default();
        let popup = PickerPopup::new(&picker, "Move to folder", &theme)
            .hint(Some("ignored hint"))
            .error(Some("there is no folder xyz"));
        let terminal = draw(NORMAL_SIZE, popup);

        let system_under_test = terminal.backend();

        assert_snapshot!(system_under_test);
    }

    #[test]
    fn popup_scrolls_to_the_selected_result() {
        let mut picker = many_folders(30);
        (0..14).for_each(|_| picker.select_next());
        let theme = Theme::default();
        let popup = PickerPopup::new(&picker, "Restore into folder", &theme).hint(Some("a hint"));
        let terminal = draw(NORMAL_SIZE, popup);

        let system_under_test = terminal.backend();

        assert_snapshot!(system_under_test);
    }

    #[test]
    fn popup_on_a_tiny_terminal_keeps_its_input() {
        let picker = picker(&["/", "Home"], "ho");
        let theme = Theme::default();
        let terminal = draw((12, 3), PickerPopup::new(&picker, "Move", &theme));

        let system_under_test = terminal.backend();

        assert_snapshot!(system_under_test);
    }

    #[rstest]
    #[case::one_cell((1, 1))]
    #[case::border_only((4, 2))]
    #[case::input_only((30, 3))]
    #[case::input_and_result((30, 4))]
    fn popup_renders_on_minimal_terminals(#[case] size: (u16, u16)) {
        let picker = many_folders(3);
        let theme = Theme::default();
        let expected = Rect::new(0, 0, size.0, size.1);

        let system_under_test = draw(
            size,
            PickerPopup::new(&picker, "Move", &theme).hint(Some("x")),
        );

        assert_eq!(system_under_test.backend().buffer().area, expected);
    }

    #[test]
    fn popup_places_the_cursor_after_the_typed_text() {
        let picker = picker(&["Home"], "ho");
        let theme = Theme::default();
        let mut terminal = draw(
            NORMAL_SIZE,
            PickerPopup::new(&picker, "Move to folder", &theme),
        );
        let expected = Position::new(14, 3);

        let system_under_test = terminal.get_cursor_position().unwrap();

        assert_eq!(system_under_test, expected);
    }

    #[test]
    fn popup_highlights_matches_and_the_selection_with_the_theme() {
        let picker = picker(&["Home", "Photos"], "ho");
        let theme = Theme::default();
        let expected_match = theme
            .style(Slot::Text)
            .patch(theme.style(Slot::SearchMatch))
            .patch(theme.style(Slot::Selection));
        let expected_plain = Style::new().fg(Color::Reset);

        let system_under_test = draw(
            NORMAL_SIZE,
            PickerPopup::new(&picker, "Move to folder", &theme),
        );

        assert_eq!(style_at(&system_under_test, 12, 4), expected_match);
        assert_eq!(style_at(&system_under_test, 15, 5), expected_plain);
    }

    #[rstest]
    #[case::nothing_selected(None, 10, 0)]
    #[case::selection_visible(Some(9), 10, 0)]
    #[case::selection_below(Some(10), 10, 1)]
    #[case::far_down(Some(29), 10, 20)]
    #[case::no_rows(Some(3), 0, 4)]
    fn first_visible_keeps_the_selection_in_view(
        #[case] selected: Option<usize>,
        #[case] visible: usize,
        #[case] expected: usize,
    ) {
        let system_under_test = first_visible(selected, visible);

        assert_eq!(system_under_test, expected);
    }
}

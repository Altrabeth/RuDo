//! The help overlay: [`render`] draws the help screen of [`Mode::Help`] over the panes.
//!
//! The overlay covers most of the terminal and lists the [`help`](crate::ui::help) rows in
//! two columns, keys and description. A scrollbar and a `top-bottom/total` indicator appear
//! when the rows do not fit, and an open or applied filter is shown in its last row.
//! [`help_overlay`] decides where everything goes; the app uses it too, so that scrolling and
//! drawing agree on the height of the viewport.

use ratatui::Frame;
use ratatui::buffer::Buffer;
use ratatui::layout::{Constraint, Layout, Margin, Rect};
use ratatui::text::Line;
use ratatui::widgets::{Block, BorderType, Clear, Scrollbar, ScrollbarOrientation, ScrollbarState};

use crate::app::help::{HelpFilter, HelpOverlay};
use crate::app::search;
use crate::app::{App, Mode};
use crate::config::theme::{Slot, Theme};
use crate::ui::help::{HelpRow, HelpSection, rows};
use crate::ui::layout::{HelpLayout, help_overlay};
use crate::ui::text_input::{TextInputView, display_width, truncated};

/// The key column is never wider than this, nor than half of the overlay.
const KEY_COLUMN_MAX_WIDTH: usize = 24;
/// Indentation of the entries below their heading.
const ENTRY_INDENT: usize = 2;
/// Space between the key column and the description.
const COLUMN_GAP: usize = 2;
const TITLE: &str = " Help ";
/// Precedes the filter text, like the key that opens the filter.
const FILTER_PROMPT: &str = search::PROMPT;
const FILTER_PLACEHOLDER: &str = "type to filter";
/// Shown when the filter matches nothing.
const NO_MATCHES: &str = "no matching keys";
/// Drawn over the right border along the visible part of the rows.
const SCROLLBAR_TRACK: &str = "\u{2502}";

/// Draws the help screen over the rest of the interface if `app` is in [`Mode::Help`].
pub fn render(app: &App, frame: &mut Frame) {
    match app.mode() {
        Mode::Normal
        | Mode::Prompt(_)
        | Mode::FolderPicker(_)
        | Mode::GoTo(_)
        | Mode::Confirm(_)
        | Mode::InlineEdit(_)
        | Mode::Search(_)
        | Mode::Visual => {}
        Mode::Help(help) => render_help(help, app.theme(), frame),
    }
}

fn render_help(help: &HelpOverlay, theme: &Theme, frame: &mut Frame) {
    let parts = help_overlay(frame.area(), help.filter().line());
    let row_count = rows(help.sections()).count();
    let viewport_height = usize::from(parts.rows.height);
    let mut block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(theme.style(Slot::PopupBorder))
        .title_style(theme.style(Slot::PopupTitle))
        .title(TITLE);
    let scrolls = row_count > viewport_height;
    if scrolls {
        block = block.title_bottom(
            Line::from(position_indicator(
                help.offset(),
                viewport_height,
                row_count,
            ))
            .right_aligned(),
        );
    }
    frame.render_widget(Clear, parts.overlay);
    frame.render_widget(block, parts.overlay);
    render_rows(help, theme, frame.buffer_mut(), parts.rows);
    if scrolls {
        render_scrollbar(
            help.offset(),
            viewport_height,
            row_count,
            theme,
            frame,
            parts,
        );
    }
    if let Some(area) = parts.filter {
        render_filter(help.filter(), theme, frame, area);
    }
}

/// Returns the indicator of the visible rows, such as ` 1-19/70 `.
pub(crate) fn position_indicator(
    offset: usize,
    viewport_height: usize,
    row_count: usize,
) -> String {
    let bottom = (offset + viewport_height).min(row_count);
    format!(" {}-{bottom}/{row_count} ", offset + 1)
}

fn render_rows(help: &HelpOverlay, theme: &Theme, buffer: &mut Buffer, area: Rect) {
    if area.is_empty() {
        return;
    }
    let width = usize::from(area.width);
    if rows(help.sections()).next().is_none() {
        let text = truncated(NO_MATCHES, width);
        buffer.set_stringn(
            area.x,
            area.y,
            text,
            width,
            theme.style(Slot::HelpDescription),
        );
        return;
    }
    let key_width = key_column_width(help.sections(), width);
    let visible_rows = rows(help.sections()).skip(help.offset());
    for (y, row) in (area.top()..area.bottom()).zip(visible_rows) {
        let row_area = Rect::new(area.x, y, area.width, 1);
        render_row(row, key_width, theme, buffer, row_area);
        if help.selected() == Some(row.key()) {
            buffer.set_style(row_area, theme.style(Slot::Selection));
        }
    }
}

/// Draws a heading over the whole row, or an entry as indented keys and description.
fn render_row(row: HelpRow<'_>, key_width: usize, theme: &Theme, buffer: &mut Buffer, area: Rect) {
    let width = usize::from(area.width);
    match row {
        HelpRow::Heading(heading) => {
            let title = heading.to_string();
            let style = theme.style(Slot::HelpHeading);
            buffer.set_stringn(area.x, area.y, truncated(&title, width), width, style);
        }
        HelpRow::Entry(entry) => {
            let cells = [
                (ENTRY_INDENT, key_width, entry.keys(), Slot::HelpKey),
                (
                    ENTRY_INDENT + key_width + COLUMN_GAP,
                    width.saturating_sub(ENTRY_INDENT + key_width + COLUMN_GAP),
                    entry.description(),
                    Slot::HelpDescription,
                ),
            ];
            for (column, cell_width, text, slot) in cells {
                let Some(x) = u16::try_from(column)
                    .ok()
                    .and_then(|column| area.x.checked_add(column))
                    .filter(|&x| x < area.right())
                else {
                    continue;
                };
                let text = truncated(text, cell_width);
                buffer.set_stringn(x, area.y, text, cell_width, theme.style(slot));
            }
        }
    }
}

/// Returns the width of the key column: that of the widest keys, capped so that the
/// descriptions keep at least half of a row `row_width` cells wide.
fn key_column_width(sections: &[HelpSection], row_width: usize) -> usize {
    let widest = sections
        .iter()
        .flat_map(HelpSection::entries)
        .map(|entry| display_width(entry.keys()))
        .max()
        .unwrap_or(0);
    widest.min(KEY_COLUMN_MAX_WIDTH).min(row_width / 2)
}

fn render_scrollbar(
    offset: usize,
    viewport_height: usize,
    row_count: usize,
    theme: &Theme,
    frame: &mut Frame,
    parts: HelpLayout,
) {
    let scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalRight)
        .begin_symbol(None)
        .end_symbol(None)
        .track_symbol(Some(SCROLLBAR_TRACK))
        .style(theme.style(Slot::PopupBorder));
    // One position per offset, so that the thumb reaches the end at the last full page.
    let mut state = ScrollbarState::new(row_count - viewport_height + 1)
        .position(offset)
        .viewport_content_length(viewport_height);
    let track = Rect {
        height: parts.rows.height,
        ..parts.overlay.inner(Margin::new(0, 1))
    };
    frame.render_stateful_widget(scrollbar, track, &mut state);
}

fn render_filter(filter: &HelpFilter, theme: &Theme, frame: &mut Frame, area: Rect) {
    let [prompt_area, input_area] = Layout::horizontal([
        Constraint::Length(FILTER_PROMPT.len().try_into().unwrap_or(u16::MAX)),
        Constraint::Fill(1),
    ])
    .areas(area);
    frame.render_widget(
        Line::styled(FILTER_PROMPT, theme.style(Slot::HelpKey)),
        prompt_area,
    );
    let view = |input| TextInputView::new(input, FILTER_PLACEHOLDER).style(theme.style(Slot::Text));
    match filter {
        HelpFilter::Off => {}
        HelpFilter::Editing(input) => view(input).render_with_cursor(frame, input_area),
        HelpFilter::Applied(input) => frame.render_widget(view(input), input_area),
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;
    use crate::config::keymap::keymap::{KeyBindings, Keymap};
    use crate::ui::help::{filter, help_sections};

    fn sections(keybindings: &str, query: &str) -> Vec<HelpSection> {
        let overrides: KeyBindings = serde_saphyr::from_str(keybindings).unwrap();
        let keymap = Keymap::new(&overrides).unwrap();
        filter(&help_sections(&keymap), query)
    }

    #[rstest]
    #[case::widest_keys("{}", "pagedown", 80, 16)]
    #[case::capped_by_half_the_row("{}", "pagedown", 20, 10)]
    #[case::capped_by_the_maximum(
        "toggle-done: [ctrl-alt-x, ctrl-alt-y, ctrl-alt-z]",
        "toggle-done",
        200,
        KEY_COLUMN_MAX_WIDTH
    )]
    #[case::nothing_shown("{}", "no such thing", 80, 0)]
    fn key_column_fits_the_widest_keys(
        #[case] keybindings: &str,
        #[case] query: &str,
        #[case] row_width: usize,
        #[case] expected: usize,
    ) {
        let sections = sections(keybindings, query);

        let system_under_test = key_column_width(&sections, row_width);

        assert_eq!(system_under_test, expected);
    }

    #[rstest]
    #[case::top(0, 19, 70, " 1-19/70 ")]
    #[case::bottom(51, 19, 70, " 52-70/70 ")]
    fn position_indicator_shows_the_visible_rows(
        #[case] offset: usize,
        #[case] viewport_height: usize,
        #[case] row_count: usize,
        #[case] expected: &str,
    ) {
        let system_under_test = position_indicator(offset, viewport_height, row_count);

        assert_eq!(system_under_test, expected);
    }
}

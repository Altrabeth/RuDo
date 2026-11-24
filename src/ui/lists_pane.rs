//! The lists pane: the workspace tree of folders and lists.
//!
//! Every row is a folder with its fold marker or a list with its `done/total` counter. The
//! counters are computed from the documents for the rendered rows only, so they follow every
//! edit without a cache.

use ratatui::Frame;
use ratatui::layout::{Margin, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Wrap};

use crate::app::lists::{TreeItem, tree_items};
use crate::app::{App, Focus};
use crate::config::keymap::action::Action;
use crate::config::keymap::keymap::Keymap;
use crate::config::theme::Slot;
use crate::storage::workspace::{Entry, List};
use crate::ui::outline::{Fold, OutlineItem, OutlineRow};

/// Title of the lists pane.
pub const TITLE: &str = "Lists";

/// Blank columns between the border and the text of a row.
const PADDING: u16 = 1;
/// Columns added per nesting level.
const INDENT_WIDTH: u16 = 2;
/// Marker of a folder that shows its entries; also used for an empty folder.
const EXPANDED_MARKER: &str = "▾ ";
/// Marker of a folder that hides its entries.
const COLLAPSED_MARKER: &str = "▸ ";
/// Marker of the active list, whose tasks the tasks pane shows.
const ACTIVE_MARKER: &str = "• ";
/// Keeps list names aligned with folder names.
const LIST_MARKER: &str = "  ";
/// Shown instead of the counter of a list that could not be loaded.
const UNREADABLE: &str = "unreadable";

/// Renders the contents of the lists pane of `app` into `area`, the inside of its border.
///
/// The scroll offset of the outline is adjusted to the real height of `area`, so that the
/// selection stays visible in any terminal size.
pub fn render(app: &App, frame: &mut Frame, area: Rect) {
    let items = tree_items(app.workspace());
    if items.is_empty() {
        render_empty_hint(app, frame, area.inner(Margin::new(PADDING, 0)));
        return;
    }
    let outline = app.lists().outline();
    let rows = outline.visible_rows(&items);
    let selected_row = rows
        .iter()
        .position(|row| Some(row.item().key()) == outline.selected());
    let offset = scroll_offset(outline.offset(), selected_row, rows.len(), area.height);
    let selection = app.theme().style(match app.focus() {
        Focus::Lists => Slot::Selection,
        Focus::Tasks => Slot::SelectionUnfocused,
    });
    for (row_area, (index, row)) in area.rows().zip(rows.iter().enumerate().skip(offset)) {
        if Some(index) == selected_row {
            frame.buffer_mut().set_style(row_area, selection);
        }
        render_row(app, frame, row_area.inner(Margin::new(PADDING, 0)), row);
    }
}

/// Returns the first row to show in a viewport `height` rows high: the outline's `offset`,
/// moved as little as needed to show the selection and to fill the viewport.
fn scroll_offset(
    offset: usize,
    selected_row: Option<usize>,
    row_count: usize,
    height: u16,
) -> usize {
    let height = usize::from(height).max(1);
    let offset = offset.min(row_count.saturating_sub(height));
    selected_row.map_or(offset, |row| {
        offset.clamp((row + 1).saturating_sub(height), row)
    })
}

/// Renders one folder or list into `area`, the row inside the padding.
fn render_row(app: &App, frame: &mut Frame, area: Rect, row: &OutlineRow<'_, TreeItem<'_>>) {
    let indent = u16::try_from(row.item().depth())
        .unwrap_or(u16::MAX)
        .saturating_mul(INDENT_WIDTH)
        .min(area.width);
    let label_area = Rect {
        x: area.x + indent,
        width: area.width - indent,
        ..area
    };
    match row.item().entry() {
        Entry::Folder(folder) => {
            let marker = match row.fold() {
                Fold::Collapsed => COLLAPSED_MARKER,
                Fold::Expanded | Fold::Leaf => EXPANDED_MARKER,
            };
            let label =
                Line::from_iter([marker, folder.name()]).style(app.theme().style(Slot::Folder));
            frame.render_widget(label, label_area);
        }
        Entry::List(list) => render_list(app, frame, label_area, list),
    }
}

/// Renders `list` with its marker and its right-aligned counter into `area`.
fn render_list(app: &App, frame: &mut Frame, area: Rect, list: &List) {
    let theme = app.theme();
    let counter = match list.content() {
        Ok(loaded) => Span::styled(
            loaded.document().progress().to_string(),
            theme.style(Slot::ListCount),
        ),
        Err(_) => Span::styled(UNREADABLE, theme.style(Slot::MessageError)),
    };
    let counter_width = u16::try_from(counter.width())
        .unwrap_or(u16::MAX)
        .min(area.width);
    let counter_area = Rect {
        x: area.right() - counter_width,
        width: counter_width,
        ..area
    };
    let label_area = Rect {
        width: area.width.saturating_sub(counter_width + 1),
        ..area
    };
    let marker = if app.lists().active() == Some(list.path()) {
        ACTIVE_MARKER
    } else {
        LIST_MARKER
    };
    let label = Line::from_iter([marker, list.name()]).style(theme.style(Slot::List));
    frame.render_widget(label, label_area);
    frame.render_widget(counter, counter_area);
}

/// Renders the hint how to create the first list into `area`.
fn render_empty_hint(app: &App, frame: &mut Frame, area: Rect) {
    let hint = Paragraph::new(empty_hint(app.keymap()))
        .style(app.theme().style(Slot::Text))
        .wrap(Wrap { trim: true });
    frame.render_widget(hint, area);
}

/// Tells which key creates a list, as the keymap actually binds it.
fn empty_hint(keymap: &Keymap) -> String {
    match keymap.keys(Action::NewList).first() {
        Some(keys) => format!("No lists yet. Press {} to create one.", keys.compact()),
        None => format!("Bind {} in your configuration", Action::NewList),
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;
    use crate::config::keymap::keymap::KeyBindings;

    #[rstest]
    #[case::default("", "No lists yet. Press o to create one.")]
    #[case::rebound("new-list: [ctrl-n, a]", "No lists yet. Press ctrl-n to create one.")]
    #[case::sequence("new-list: [n l]", "No lists yet. Press nl to create one.")]
    #[case::unbound("new-list: []", "Bind new-list in your configuration")]
    fn empty_hint_names_the_key_bound_to_new_list(#[case] yaml: &str, #[case] expected: &str) {
        let overrides: KeyBindings = if yaml.is_empty() {
            KeyBindings::default()
        } else {
            serde_saphyr::from_str(yaml).unwrap()
        };
        let keymap = Keymap::new(&overrides).unwrap();

        let system_under_test = empty_hint(&keymap);

        assert_eq!(system_under_test, expected);
    }

    #[rstest]
    #[case::keeps_a_valid_offset(3, Some(5), 20, 4, 3)]
    #[case::scrolls_down_to_the_selection(0, Some(9), 20, 4, 6)]
    #[case::scrolls_up_to_the_selection(8, Some(2), 20, 4, 2)]
    #[case::fills_a_taller_viewport(9, Some(29), 30, 25, 5)]
    #[case::shows_all_rows_that_fit(5, Some(0), 3, 10, 0)]
    #[case::without_selection(2, None, 20, 4, 2)]
    #[case::empty_viewport(0, Some(3), 20, 0, 3)]
    fn scroll_offset_shows_the_selection(
        #[case] offset: usize,
        #[case] selected_row: Option<usize>,
        #[case] row_count: usize,
        #[case] height: u16,
        #[case] expected: usize,
    ) {
        let system_under_test = scroll_offset(offset, selected_row, row_count, height);

        assert_eq!(system_under_test, expected);
    }
}

//! The lists pane: the workspace tree of folders and lists.
//!
//! Every row is a folder with its fold marker or a list with its `done/total` counter, and the
//! last row the bin with the number of entries in it. The counters are computed from the
//! documents for the rendered rows only, so they follow every edit without a cache. While the
//! bin is open, the pane shows its entries with their original path and deletion date instead.

use std::borrow::Cow;

use jiff::tz::TimeZone;
use ratatui::Frame;
use ratatui::layout::{Margin, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};

use crate::app::bin::{BinView, bin_items};
use crate::app::entry_visibility::HiddenEntries;
use crate::app::lists::{TreeItem, TreeRow};
use crate::app::{App, Focus};
use crate::config::ListOrder;
use crate::config::keymap::action::Action;
use crate::config::keymap::keymap::Keymap;
use crate::config::theme::Slot;
use crate::paths::portable;
use crate::storage::bin::BinEntry;
use crate::storage::workspace::{Entry, List};
use crate::ui::highlight::Highlight;
use crate::ui::outline::{FilterRole, Fold, OutlineItem, OutlineRow};
use crate::ui::pane_message::{NO_MATCHES, key_sentence, render_message};
use crate::ui::tasks_pane::TITLE_SEPARATOR;

/// Title of the lists pane.
pub const TITLE: &str = "Lists";
/// Title of the lists pane while it shows the bin, and the name of its row in the tree.
pub const BIN_TITLE: &str = "Bin";
/// Shown instead of the deletion date of an entry that another program put into the bin.
const UNKNOWN_DATE: &str = "unknown";

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
/// Added to the title while hidden entries are shown.
const HIDDEN_SHOWN: &str = "hidden shown";

/// Returns the title of the lists pane of `app`: [`TITLE`] with the order unless it is by name,
/// followed by `hidden shown` while hidden entries are shown and by the search filter while
/// one is active, such as `Lists by due  hidden shown  /milk`, or [`BIN_TITLE`] while the bin
/// is open.
#[must_use]
pub fn title(app: &App) -> Cow<'_, str> {
    if app.lists().bin_view().is_some() {
        return Cow::Borrowed(BIN_TITLE);
    }
    let filter = app.lists().filter();
    let order = order_label(app.lists().order());
    let hidden_shown = match app.hidden_entries() {
        HiddenEntries::Shown => Some(HIDDEN_SHOWN),
        HiddenEntries::Hidden => None,
    };
    if !filter.is_active() && hidden_shown.is_none() && order.is_none() {
        return Cow::Borrowed(TITLE);
    }
    let mut title = TITLE.to_owned();
    if let Some(order) = order {
        title.push(' ');
        title.push_str(order);
    }
    if let Some(hidden_shown) = hidden_shown {
        title.push_str(TITLE_SEPARATOR);
        title.push_str(hidden_shown);
    }
    if filter.is_active() {
        title.push_str(TITLE_SEPARATOR);
        title.push_str(&filter.to_string());
    }
    Cow::Owned(title)
}

/// Names `order` after the title of the pane, such as `by due`; the order by name goes
/// without saying.
const fn order_label(order: ListOrder) -> Option<&'static str> {
    match order {
        ListOrder::Name => None,
        ListOrder::Due => Some("by due"),
        ListOrder::Priority => Some("by priority"),
        ListOrder::OpenTasks => Some("by open tasks"),
        ListOrder::TotalTasks => Some("by total tasks"),
    }
}

/// Renders the contents of the lists pane of `app` into `area`, the inside of its border.
///
/// The rows start at the scroll offset of the outline, which the app keeps valid for the
/// height of `area` because it derives the viewport from the same layout. Rows selected in
/// visual mode are drawn with [`Slot::VisualSelection`], except folders shown only because
/// they contain a search match and the bin, which no selection acts on.
pub fn render(app: &App, frame: &mut Frame, area: Rect) {
    if let Some(view) = app.lists().bin_view() {
        render_bin(app, frame, area, view);
        return;
    }
    let items = app.lists().items(app.workspace());
    if items.is_empty() {
        let text_area = area.inner(Margin::new(PADDING, 0));
        if app.lists().filter().is_active() {
            render_message(app, frame, text_area, NO_MATCHES, Slot::Text);
        } else {
            render_empty_hint(app, frame, text_area);
        }
        return;
    }
    let outline = app.lists().outline();
    let rows = outline.visible_rows(&items);
    let selected_row = rows
        .iter()
        .position(|row| Some(row.item().key()) == outline.selected());
    let selection = app.theme().style(match app.focus() {
        Focus::Lists => Slot::Selection,
        Focus::Tasks => Slot::SelectionUnfocused,
    });
    let visual_rows = outline.selection_rows(&rows).unwrap_or_default();
    let visual_selection = app.theme().style(Slot::VisualSelection);
    for (row_area, (index, row)) in area
        .rows()
        .zip(rows.iter().enumerate().skip(outline.offset()))
    {
        if visual_rows.contains(&index)
            && row.item().role() == FilterRole::Match
            && row.item().entry().is_some()
        {
            frame.buffer_mut().set_style(row_area, visual_selection);
        }
        if Some(index) == selected_row {
            frame.buffer_mut().set_style(row_area, selection);
        }
        render_row(app, frame, row_area.inner(Margin::new(PADDING, 0)), row);
    }
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
    let entry = match row.item().row() {
        TreeRow::Entry(entry) => entry,
        TreeRow::Bin(entries) => {
            let label =
                Line::from_iter([LIST_MARKER, BIN_TITLE]).style(app.theme().style(Slot::Bin));
            let counter = Span::styled(
                entries.len().to_string(),
                app.theme().style(Slot::ListCount),
            );
            render_with_counter(frame, label_area, label, counter);
            return;
        }
    };
    match entry {
        Entry::Folder(folder) => {
            let marker = match row.fold() {
                Fold::Collapsed => COLLAPSED_MARKER,
                Fold::Expanded | Fold::Leaf => EXPANDED_MARKER,
            };
            let slot = match row.item().role() {
                FilterRole::Match => Slot::Folder,
                FilterRole::Context => Slot::FilterContext,
            };
            let style = label_style(app, entry, slot);
            let label = Line::from_iter([marker, folder.name()]).style(style);
            frame.render_widget(label, label_area);
        }
        Entry::List(list) => {
            let style = label_style(app, entry, Slot::List);
            render_list(app, frame, label_area, list, style);
        }
    }
}

/// Returns the style of the name of `entry` drawn with `slot`, dimmed with
/// [`Slot::HiddenEntry`] if the entry is hidden.
fn label_style(app: &App, entry: &Entry, slot: Slot) -> Style {
    let style = app.theme().style(slot);
    if entry.is_hidden() {
        style.patch(app.theme().style(Slot::HiddenEntry))
    } else {
        style
    }
}

/// Renders `label` into `area` with `counter` aligned to its right edge.
fn render_with_counter(frame: &mut Frame, area: Rect, label: Line<'_>, counter: Span<'_>) {
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
    frame.render_widget(label, label_area);
    frame.render_widget(counter, counter_area);
}

/// Renders `list` with its marker and its right-aligned counter into `area`, its name in
/// `style`.
fn render_list(app: &App, frame: &mut Frame, area: Rect, list: &List, style: Style) {
    let theme = app.theme();
    let counter = match list.content() {
        Ok(loaded) => Span::styled(
            loaded.document().progress().to_string(),
            theme.style(Slot::ListCount),
        ),
        Err(_) => Span::styled(UNREADABLE, theme.style(Slot::MessageError)),
    };
    let marker = if app.lists().active() == Some(list.path()) {
        ACTIVE_MARKER
    } else {
        LIST_MARKER
    };
    let highlight = Highlight::new(app.lists().filter().query(), list.name(), theme);
    let mut spans = vec![Span::raw(marker)];
    highlight.push_spans(&mut spans, list.name(), 0, Style::new());
    let label = Line::from(spans).style(style);
    render_with_counter(frame, area, label, counter);
}

/// Renders the entries of the bin of `app` that `view` scrolled to into `area`, the selected
/// one highlighted and those selected in visual mode drawn with [`Slot::VisualSelection`]; the
/// focus never leaves the lists pane while the bin is open.
fn render_bin(app: &App, frame: &mut Frame, area: Rect, view: &BinView) {
    let items = bin_items(app.workspace());
    let outline = view.outline();
    let rows = outline.visible_rows(&items);
    let selection = app.theme().style(Slot::Selection);
    let visual_rows = outline.selection_rows(&rows).unwrap_or_default();
    let visual_selection = app.theme().style(Slot::VisualSelection);
    for (row_area, (index, row)) in area
        .rows()
        .zip(rows.iter().enumerate().skip(outline.offset()))
    {
        if visual_rows.contains(&index) {
            frame.buffer_mut().set_style(row_area, visual_selection);
        }
        if Some(row.item().key()) == outline.selected() {
            frame.buffer_mut().set_style(row_area, selection);
        }
        let text_area = row_area.inner(Margin::new(PADDING, 0));
        render_bin_entry(app, frame, text_area, row.item().entry());
    }
}

/// Renders `entry` of the bin into `area`: its original path, or its path in the bin if that is
/// unknown, with the date it was deleted aligned to the right.
fn render_bin_entry(app: &App, frame: &mut Frame, area: Rect, entry: &BinEntry) {
    let theme = app.theme();
    let (marker, slot) = match entry.entry() {
        Entry::Folder(_) => (COLLAPSED_MARKER, Slot::Folder),
        Entry::List(_) => (LIST_MARKER, Slot::List),
    };
    let path = entry.original().unwrap_or(entry.location());
    let name = match entry.entry() {
        Entry::Folder(_) => portable(path).to_string(),
        Entry::List(_) => portable(&path.with_extension("")).to_string(),
    };
    let label = Line::from_iter([Span::raw(marker), Span::raw(name)]).style(theme.style(slot));
    let date = entry
        .deleted_at()
        .map_or(Cow::Borrowed(UNKNOWN_DATE), |deleted_at| {
            Cow::Owned(
                deleted_at
                    .to_zoned_datetime(TimeZone::system())
                    .date()
                    .to_string(),
            )
        });
    render_with_counter(
        frame,
        area,
        label,
        Span::styled(date, theme.style(Slot::ListCount)),
    );
}

/// Renders the hint how to create the first list into `area`.
fn render_empty_hint(app: &App, frame: &mut Frame, area: Rect) {
    render_message(app, frame, area, &empty_hint(app.keymap()), Slot::Text);
}

/// Tells which key creates a list, as the keymap actually binds it.
fn empty_hint(keymap: &Keymap) -> String {
    key_sentence(keymap, Action::NewList, |keys| {
        format!("No lists yet. Press {keys} to create one.")
    })
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
}

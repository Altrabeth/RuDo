//! The tasks pane: the task tree of the active list.
//!
//! Every row shows the tree guides of its ancestors, a fold marker, the checkbox and the
//! description, styled by the status of the task. [`TaskRow`] builds the content of a row
//! from its task alone, so every part a task shows is added in one place.

use std::borrow::Cow;

use ratatui::Frame;
use ratatui::layout::{Margin, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};

use crate::app::tasks::{TaskItem, task_items};
use crate::app::{App, Focus};
use crate::config::keymap::action::Action;
use crate::config::keymap::keymap::Keymap;
use crate::config::theme::{Slot, Theme};
use crate::markdown::document::TaskId;
use crate::markdown::task_line::TaskStatus;
use crate::ui::outline::{Fold, Outline, OutlineItem, OutlineRow};
use crate::ui::pane_message::{key_sentence, render_message};
use crate::ui::text_input::{display_width, truncated};

/// Title of the tasks pane while no list is open; otherwise the title is the list's name and
/// its `done/total` counter, see [`title`].
pub const TITLE: &str = "Tasks";

/// Blank columns between the border and the text of a row.
const PADDING: u16 = 1;
/// Drawn once per ancestor of a task.
const TREE_GUIDE: &str = "│ ";
/// Marker of a task that shows its subtasks.
const EXPANDED_MARKER: &str = "▾ ";
/// Marker of a task that hides its subtasks.
const COLLAPSED_MARKER: &str = "▸ ";
/// Keeps the checkboxes of tasks without subtasks aligned with the others.
const LEAF_MARKER: &str = "  ";
/// Separates the list name from its counter in the title.
const TITLE_SEPARATOR: &str = "  ";
/// Columns that tree guides never take from a row: the fold marker, the checkbox and the
/// first ten cells of the description.
const MIN_TEXT_WIDTH: usize = 16;

/// Returns the title of the tasks pane of `app`: the name of the active list with its
/// `done/total` counter, or [`TITLE`] while no list is open.
#[must_use]
pub fn title(app: &App) -> Cow<'_, str> {
    let Some(list) = app.active_list() else {
        return Cow::Borrowed(TITLE);
    };
    match list.content() {
        Ok(loaded) => Cow::Owned(format!(
            "{}{TITLE_SEPARATOR}{}",
            list.name(),
            loaded.document().progress()
        )),
        // The pane itself shows why the list could not be loaded.
        Err(_) => Cow::Borrowed(list.name()),
    }
}

/// Renders the contents of the tasks pane of `app` into `area`, the inside of its border.
///
/// The rows start at the scroll offset of the outline, which the app keeps valid for the
/// height of `area` because it derives the viewport from the same layout.
pub fn render(app: &App, frame: &mut Frame, area: Rect) {
    let text_area = area.inner(Margin::new(PADDING, 0));
    let Some(list) = app.active_list() else {
        render_message(
            app,
            frame,
            text_area,
            &no_list_hint(app.keymap()),
            Slot::Text,
        );
        return;
    };
    let loaded = match list.content() {
        Ok(loaded) => loaded,
        Err(error) => {
            render_message(
                app,
                frame,
                text_area,
                &error.to_string(),
                Slot::MessageError,
            );
            return;
        }
    };
    let items = task_items(loaded.document());
    if items.is_empty() {
        render_message(
            app,
            frame,
            text_area,
            &no_tasks_hint(app.keymap()),
            Slot::Text,
        );
        return;
    }
    let fallback = Outline::new();
    let outline = app.task_outline().unwrap_or(&fallback);
    render_rows(app, frame, area, outline, &items);
}

/// Renders the visible `items` of `outline` from its scroll offset on, highlighting the
/// selected one.
fn render_rows(
    app: &App,
    frame: &mut Frame,
    area: Rect,
    outline: &Outline<TaskId>,
    items: &[TaskItem<'_>],
) {
    let theme = app.theme();
    let selection = theme.style(match app.focus() {
        Focus::Tasks => Slot::Selection,
        Focus::Lists => Slot::SelectionUnfocused,
    });
    for (row_area, row) in area
        .rows()
        .zip(outline.visible_rows(items).iter().skip(outline.offset()))
    {
        if Some(row.item().key()) == outline.selected() {
            frame.buffer_mut().set_style(row_area, selection);
        }
        let text_area = row_area.inner(Margin::new(PADDING, 0));
        let line = TaskRow::new(row, theme).into_line(usize::from(text_area.width));
        frame.render_widget(line, text_area);
    }
}

/// Tells which key opens a list, as the keymap actually binds it.
fn no_list_hint(keymap: &Keymap) -> String {
    key_sentence(keymap, Action::Open, |keys| {
        format!("Select a list and press {keys}")
    })
}

/// Tells which key adds the first task, as the keymap actually binds it.
fn no_tasks_hint(keymap: &Keymap) -> String {
    key_sentence(keymap, Action::NewTaskBelow, |keys| {
        format!("No tasks yet. Press {keys} to add one.")
    })
}

/// The content of one row of the task tree, as styled spans.
///
/// Every part a task shows is added in [`TaskRow::new`]. [`TaskRow::into_line`] fits the row
/// into the width of the pane: deep nesting loses tree guides before the text becomes
/// invisible, and the description is cut with an ellipsis.
#[derive(Debug, Clone, PartialEq)]
pub struct TaskRow<'a> {
    /// Number of ancestors, each drawn as a tree guide.
    depth: usize,
    guide_style: Style,
    /// Spans that are always shown in full: fold marker, checkbox and separator.
    lead: Vec<Span<'a>>,
    /// The description without inline fields; cut if the row is too narrow.
    description: Span<'a>,
}

impl<'a> TaskRow<'a> {
    /// Builds the row of the task in `row`, styled with `theme`.
    #[must_use]
    pub fn new(row: &OutlineRow<'_, TaskItem<'a>>, theme: &Theme) -> Self {
        let task = row.item().task();
        let guide_style = theme.style(Slot::TreeGuide);
        let status_style = theme.style(status_slot(task.status()));
        let marker = match row.fold() {
            Fold::Leaf => LEAF_MARKER,
            Fold::Expanded => EXPANDED_MARKER,
            Fold::Collapsed => COLLAPSED_MARKER,
        };
        Self {
            depth: row.item().depth(),
            guide_style,
            lead: vec![
                Span::styled(marker, guide_style),
                Span::styled(checkbox(task.status()), status_style),
                Span::raw(" "),
            ],
            description: Span::styled(task.description(), status_style),
        }
    }

    /// Returns the row fitted into `width` cells.
    ///
    /// Tree guides are drawn only as long as sixteen cells remain for the fold marker, the
    /// checkbox and the start of the description, and the description is cut with an
    /// ellipsis where the row ends.
    #[must_use]
    pub fn into_line(self, width: usize) -> Line<'a> {
        let guide_width = display_width(TREE_GUIDE);
        let guides = self
            .depth
            .min(width.saturating_sub(MIN_TEXT_WIDTH) / guide_width);
        let lead_width: usize = guides * guide_width
            + self
                .lead
                .iter()
                .map(|span| display_width(&span.content))
                .sum::<usize>();
        let description_width = width.saturating_sub(lead_width);
        let description = match truncated(&self.description.content, description_width) {
            Cow::Borrowed(text) if text.len() == self.description.content.len() => self.description,
            cut => Span::styled(cut.into_owned(), self.description.style),
        };
        let mut spans = Vec::with_capacity(self.lead.len() + 2);
        spans.push(Span::styled(TREE_GUIDE.repeat(guides), self.guide_style));
        spans.extend(self.lead);
        spans.push(description);
        Line::from(spans)
    }
}

/// Returns the slot that styles the checkbox and description of a task with `status`.
fn status_slot(status: TaskStatus) -> Slot {
    match status {
        TaskStatus::Todo | TaskStatus::Other(_) => Slot::Text,
        TaskStatus::Done => Slot::TaskDone,
        TaskStatus::InProgress => Slot::TaskInProgress,
        TaskStatus::Cancelled => Slot::TaskCancelled,
    }
}

/// Returns the checkbox of `status` as written in Markdown, such as `[x]`.
fn checkbox(status: TaskStatus) -> Cow<'static, str> {
    match status {
        TaskStatus::Todo => Cow::Borrowed("[ ]"),
        TaskStatus::Done => Cow::Borrowed("[x]"),
        TaskStatus::InProgress => Cow::Borrowed("[/]"),
        TaskStatus::Cancelled => Cow::Borrowed("[-]"),
        TaskStatus::Other(character) => Cow::Owned(format!("[{character}]")),
    }
}

#[cfg(test)]
mod tests {
    use ratatui::style::{Color, Modifier};
    use rstest::rstest;
    use strum::IntoEnumIterator;

    use super::*;
    use crate::config::keymap::keymap::KeyBindings;
    use crate::markdown::document::TodoDocument;
    use crate::ui::outline::{Outcome, OutlineCommand};

    const HEIGHT: usize = 10;

    fn keymap(yaml: &str) -> Keymap {
        let overrides: KeyBindings = if yaml.is_empty() {
            KeyBindings::default()
        } else {
            serde_saphyr::from_str(yaml).unwrap()
        };
        Keymap::new(&overrides).unwrap()
    }

    /// Returns the text of row `row` of `document` after `commands`, fitted into `width`
    /// cells.
    fn row_text(document: &str, commands: &[OutlineCommand], row: usize, width: usize) -> String {
        let document = TodoDocument::parse(document);
        let items = task_items(&document);
        let mut outline = Outline::new();
        outline.refresh(&items, HEIGHT);
        for &command in commands {
            assert_eq!(outline.apply(&items, command, HEIGHT), Outcome::Changed);
        }
        let rows = outline.visible_rows(&items);
        TaskRow::new(&rows[row], &Theme::default())
            .into_line(width)
            .to_string()
    }

    #[rstest]
    #[case::leaf("- [ ] a\n", &[], 0, "  [ ] a")]
    #[case::expanded("- [ ] a\n\t- [ ] b\n", &[], 0, "▾ [ ] a")]
    #[case::collapsed("- [ ] a\n\t- [ ] b\n", &[OutlineCommand::Collapse], 0, "▸ [ ] a")]
    #[case::subtask("- [ ] a\n\t- [x] b\n", &[], 1, "│   [x] b")]
    #[case::inline_fields_hidden("- [ ] a [due:: 2026-03-20] #tag\n", &[], 0, "  [ ] a #tag")]
    fn row_shows_guides_marker_checkbox_and_description(
        #[case] document: &str,
        #[case] commands: &[OutlineCommand],
        #[case] row: usize,
        #[case] expected: &str,
    ) {
        let system_under_test = row_text(document, commands, row, 40);

        assert_eq!(system_under_test, expected);
    }

    #[rstest]
    #[case::all_guides(20, "│ │   [ ] cccc")]
    #[case::guides_capped(18, "│   [ ] cccc")]
    #[case::no_guides(16, "  [ ] cccc")]
    #[case::description_cut(8, "  [ ] c…")]
    #[case::only_the_lead(6, "  [ ] ")]
    fn narrow_rows_drop_guides_before_cutting_the_description(
        #[case] width: usize,
        #[case] expected: &str,
    ) {
        let document = "- [ ] a\n\t- [ ] b\n\t\t- [ ] cccc\n";

        let system_under_test = row_text(document, &[], 2, width);

        assert_eq!(system_under_test, expected);
    }

    #[test]
    fn every_status_has_its_checkbox() {
        let expected: Vec<String> = TaskStatus::iter()
            .map(|status| format!("[{}]", status.as_char()))
            .collect();

        let system_under_test: Vec<Cow<'static, str>> = TaskStatus::iter().map(checkbox).collect();

        assert_eq!(system_under_test, expected);
    }

    #[rstest]
    #[case::todo(TaskStatus::Todo, Style::new())]
    #[case::other(TaskStatus::Other('?'), Style::new())]
    #[case::done(TaskStatus::Done, Style::new().fg(Color::DarkGray))]
    #[case::in_progress(TaskStatus::InProgress, Style::new().fg(Color::Yellow))]
    #[case::cancelled(
        TaskStatus::Cancelled,
        Style::new().fg(Color::DarkGray).add_modifier(Modifier::CROSSED_OUT)
    )]
    fn status_picks_its_theme_slot(#[case] status: TaskStatus, #[case] expected: Style) {
        let system_under_test = Theme::default().style(status_slot(status));

        assert_eq!(system_under_test, expected);
    }

    #[rstest]
    #[case::default("", "Select a list and press enter")]
    #[case::rebound("open: [ctrl-o]", "Select a list and press ctrl-o")]
    #[case::unbound("open: []", "Bind open in your configuration")]
    fn no_list_hint_names_the_key_bound_to_open(#[case] yaml: &str, #[case] expected: &str) {
        let keymap = keymap(yaml);

        let system_under_test = no_list_hint(&keymap);

        assert_eq!(system_under_test, expected);
    }

    #[rstest]
    #[case::default("", "No tasks yet. Press o to add one.")]
    #[case::sequence("new-task-below: [n t]", "No tasks yet. Press nt to add one.")]
    #[case::unbound("new-task-below: []", "Bind new-task-below in your configuration")]
    fn no_tasks_hint_names_the_key_bound_to_new_task_below(
        #[case] yaml: &str,
        #[case] expected: &str,
    ) {
        let keymap = keymap(yaml);

        let system_under_test = no_tasks_hint(&keymap);

        assert_eq!(system_under_test, expected);
    }
}

//! The tasks pane: the task tree of the active list.
//!
//! Every row shows the tree guides of its ancestors, a fold marker, the checkbox and the
//! description, styled by the status of the task with its tags highlighted. [`TaskRow`] builds the content of a row
//! from its task alone, so every part a task shows is added in one place. Metadata such as the
//! recurrence rule, the priority and the due date is aligned to the right edge of the row.
//!
//! While a new task is typed in [`Mode::InlineEdit`], a placeholder row with the text input
//! is shown where the task will be inserted; the document itself is only changed on submit.
//! While an existing task is edited, its row shows the text input in place of its
//! description.

use std::borrow::Cow;

use jiff::civil::Date;
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Margin, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};

use crate::app::task_input::InlineEditKind;
use crate::app::task_visibility::CompletedTasks;
use crate::app::tasks::{TaskItem, visible_task_items};
use crate::app::{App, Focus, Mode};
use crate::config::keymap::action::Action;
use crate::config::keymap::keymap::Keymap;
use crate::config::theme::{Slot, Theme};
use crate::domain::date::{DueClass, RelativeDue};
use crate::domain::priority::Priority;
use crate::domain::tags;
use crate::markdown::document::{InsertPosition, TaskId};
use crate::markdown::task_line::{FieldKey, TaskLine, TaskStatus};
use crate::ui::outline::{Fold, Outline, OutlineItem, OutlineRow};
use crate::ui::pane_message::{key_sentence, render_message};
use crate::ui::text_input::{TextInput, TextInputView, display_width, fitted};

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
/// Separates the parts of the title: list name, counter and [`COMPLETED_HIDDEN`].
const TITLE_SEPARATOR: &str = "  ";
/// Ends the title while completed tasks are hidden.
const COMPLETED_HIDDEN: &str = "completed hidden";
/// Columns that tree guides never take from a row: the fold marker, the checkbox and the
/// first ten cells of the description.
const MIN_TEXT_WIDTH: usize = 16;
/// Separates the description from the trailing metadata and its parts from each other.
const METADATA_GAP: &str = " ";

/// Returns the title of the tasks pane of `app`: the name of the active list with its
/// `done/total` counter of all tasks, followed by `completed hidden` while completed tasks
/// are hidden, or [`TITLE`] while no list is open.
#[must_use]
pub fn title(app: &App) -> Cow<'_, str> {
    let Some(list) = app.active_list() else {
        return Cow::Borrowed(TITLE);
    };
    match list.content() {
        Ok(loaded) => {
            let mut title = format!(
                "{}{TITLE_SEPARATOR}{}",
                list.name(),
                loaded.document().progress()
            );
            match app.completed_tasks() {
                CompletedTasks::Shown => {}
                CompletedTasks::Hidden => {
                    title.push_str(TITLE_SEPARATOR);
                    title.push_str(COMPLETED_HIDDEN);
                }
            }
            Cow::Owned(title)
        }
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
    let items = visible_task_items(loaded.document(), app.completed_tasks());
    let inline_edit = inline_edit(app);
    let new_task = inline_edit.and_then(|(kind, input)| match kind {
        InlineEditKind::New(position) => Some((position, input)),
        InlineEditKind::Existing(_) => None,
    });
    let edited_task = inline_edit.and_then(|(kind, input)| match kind {
        InlineEditKind::Existing(id) => Some((id, input)),
        InlineEditKind::New(_) => None,
    });
    if items.is_empty() && new_task.is_none() {
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
    let visible_rows = outline.visible_rows(&items);
    let placeholder = new_task.map(|(position, input)| {
        let (row, depth) = placeholder_slot(&visible_rows, position);
        (row, PaneRow::NewTask { depth, input })
    });
    let offset = match &placeholder {
        Some((row, _)) => offset_showing(outline.offset(), *row, usize::from(area.height)),
        None => outline.offset(),
    };
    let mut rows: Vec<PaneRow<'_, '_>> = visible_rows
        .into_iter()
        .map(|row| match edited_task {
            Some((id, input)) if *row.item().key() == id => PaneRow::EditedTask { row, input },
            Some(_) | None => PaneRow::Task(row),
        })
        .collect();
    if let Some((row, placeholder)) = placeholder {
        rows.insert(row, placeholder);
    }
    let first_row = offset.min(rows.len());
    render_rows(app, frame, area, outline.selected(), &rows[first_row..]);
}

/// A row of the tasks pane: a task, a task whose text is edited, or the placeholder of a new
/// task while its text is typed.
enum PaneRow<'a, 'document> {
    Task(OutlineRow<'a, TaskItem<'document>>),
    EditedTask {
        row: OutlineRow<'a, TaskItem<'document>>,
        input: &'a TextInput,
    },
    NewTask {
        depth: usize,
        input: &'a TextInput,
    },
}

/// Returns which task the inline editor of `app` is for and its text, or `None` if no inline
/// editor is open.
fn inline_edit(app: &App) -> Option<(InlineEditKind, &TextInput)> {
    match app.mode() {
        Mode::InlineEdit(edit) => Some((edit.kind(), edit.input())),
        Mode::Normal | Mode::Help(_) | Mode::Prompt(_) | Mode::Confirm(_) => None,
    }
}

/// Returns the index among `rows` before which a new task at `position` is shown, and its
/// depth.
///
/// A task below its reference task or as its subtask goes after the visible block of the
/// reference task, which ends at the task itself if it is folded. A reference task that is
/// not visible is treated like [`InsertPosition::End`].
fn placeholder_slot(
    rows: &[OutlineRow<'_, TaskItem<'_>>],
    position: InsertPosition,
) -> (usize, usize) {
    let row_of = |id: TaskId| rows.iter().position(|row| *row.item().key() == id);
    let depth_of = |row: usize| rows[row].item().depth();
    let end_of_block = |row: usize| {
        rows[row + 1..]
            .iter()
            .position(|next| next.item().depth() <= depth_of(row))
            .map_or(rows.len(), |after| row + 1 + after)
    };
    let slot = match position {
        InsertPosition::Below(id) => row_of(id).map(|row| (end_of_block(row), depth_of(row))),
        InsertPosition::Above(id) => row_of(id).map(|row| (row, depth_of(row))),
        InsertPosition::LastChild(id) => {
            row_of(id).map(|row| (end_of_block(row), depth_of(row) + 1))
        }
        InsertPosition::End => None,
    };
    slot.unwrap_or((rows.len(), 0))
}

/// Returns the scroll offset closest to `offset` at which `row` is inside a viewport
/// `height` rows high.
fn offset_showing(offset: usize, row: usize, height: usize) -> usize {
    offset.min(row).max((row + 1).saturating_sub(height))
}

/// Renders `rows` from the top of `area`, highlighting the `selected` task.
fn render_rows(
    app: &App,
    frame: &mut Frame,
    area: Rect,
    selected: Option<&TaskId>,
    rows: &[PaneRow<'_, '_>],
) {
    let theme = app.theme();
    let selection = theme.style(match app.focus() {
        Focus::Tasks => Slot::Selection,
        Focus::Lists => Slot::SelectionUnfocused,
    });
    for (row_area, row) in area.rows().zip(rows) {
        let text_area = row_area.inner(Margin::new(PADDING, 0));
        let width = usize::from(text_area.width);
        let mut highlight_selected = |row: &OutlineRow<'_, TaskItem<'_>>| {
            if Some(row.item().key()) == selected {
                frame.buffer_mut().set_style(row_area, selection);
            }
        };
        match row {
            PaneRow::Task(row) => {
                highlight_selected(row);
                frame.render_widget(
                    TaskRow::new(row, theme, app.today()).into_line(width),
                    text_area,
                );
            }
            PaneRow::EditedTask { row, input } => {
                highlight_selected(row);
                render_input_row(
                    frame,
                    text_area,
                    TaskRow::new(row, theme, app.today()).lead_only(),
                    input,
                    theme,
                );
            }
            PaneRow::NewTask { depth, input } => {
                render_input_row(
                    frame,
                    text_area,
                    TaskRow::new_task(*depth, theme),
                    input,
                    theme,
                );
            }
        }
    }
}

/// Renders `row`, a task row without description, into `area`, followed by the `input` with
/// its cursor.
fn render_input_row(
    frame: &mut Frame,
    area: Rect,
    row: TaskRow<'_>,
    input: &TextInput,
    theme: &Theme,
) {
    let lead = row.into_line(usize::from(area.width));
    let lead_width = u16::try_from(lead.width()).unwrap_or(u16::MAX);
    let [lead_area, input_area] =
        Layout::horizontal([Constraint::Length(lead_width), Constraint::Fill(1)]).areas(area);
    frame.render_widget(lead, lead_area);
    TextInputView::new(input, "")
        .style(theme.style(Slot::Text))
        .render_with_cursor(frame, input_area);
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
/// invisible, the description is cut with an ellipsis, and the trailing metadata is aligned to
/// the right edge unless the row is too narrow for it.
#[derive(Debug, Clone, PartialEq)]
pub struct TaskRow<'a> {
    /// Number of ancestors, each drawn as a tree guide.
    depth: usize,
    guide_style: Style,
    /// Spans that are always shown in full: fold marker, checkbox and separator.
    lead: Vec<Span<'a>>,
    /// The description without inline fields; cut if the row is too narrow.
    description: Description<'a>,
    /// Right-aligned metadata such as the priority and the due date.
    metadata: TrailingMetadata<'a>,
}

impl<'a> TaskRow<'a> {
    /// Builds the row of the task in `row`, styled with `theme`, with dates relative to
    /// `today`.
    #[must_use]
    pub fn new(row: &OutlineRow<'_, TaskItem<'a>>, theme: &Theme, today: Date) -> Self {
        let task = row.item().task();
        let marker = match row.fold() {
            Fold::Leaf => LEAF_MARKER,
            Fold::Expanded => EXPANDED_MARKER,
            Fold::Collapsed => COLLAPSED_MARKER,
        };
        let depth = row.item().depth();
        let metadata = TrailingMetadata {
            recurrence: recurrence_part(task, theme),
            priority: priority_part(task, theme),
            due: due_part(task, theme, today),
        };
        Self::from_parts(
            depth,
            marker,
            task.status(),
            task.description(),
            metadata,
            theme,
        )
    }

    /// Builds the row of a new task `depth` levels deep without a description; the text input
    /// drawn after the row shows the typed text instead.
    fn new_task(depth: usize, theme: &Theme) -> Self {
        Self::from_parts(
            depth,
            LEAF_MARKER,
            TaskStatus::Todo,
            "",
            TrailingMetadata::default(),
            theme,
        )
    }

    /// Returns only the lead of the row (tree guides, fold marker and checkbox), for a task
    /// whose text is edited in the text input drawn after it. The description and the trailing
    /// metadata are left out, so that the input gets the rest of the row.
    fn lead_only(mut self) -> Self {
        self.description.text = "";
        self.metadata = TrailingMetadata::default();
        self
    }

    fn from_parts(
        depth: usize,
        marker: &'static str,
        status: TaskStatus,
        description: &'a str,
        metadata: TrailingMetadata<'a>,
        theme: &Theme,
    ) -> Self {
        let guide_style = theme.style(Slot::TreeGuide);
        let status_style = theme.style(status_slot(status));
        Self {
            depth,
            guide_style,
            lead: vec![
                Span::styled(marker, guide_style),
                Span::styled(checkbox(status), status_style),
                Span::raw(" "),
            ],
            description: Description {
                text: description,
                style: status_style,
                tag_style: theme.style(Slot::Tag),
            },
            metadata,
        }
    }

    /// Returns the row fitted into `width` cells.
    ///
    /// Tree guides are drawn only as long as sixteen cells remain for the fold marker, the
    /// checkbox and the start of the description, and the description is cut with an
    /// ellipsis where the trailing metadata begins, or where the row ends if the metadata does
    /// not fit next to the lead.
    #[must_use]
    pub fn into_line(self, width: usize) -> Line<'a> {
        let guide_width = display_width(TREE_GUIDE);
        let guides = self
            .depth
            .min(width.saturating_sub(MIN_TEXT_WIDTH) / guide_width);
        let lead_width = guides * guide_width + spans_width(&self.lead);
        let metadata = self.metadata.into_spans();
        let metadata_width = spans_width(&metadata);
        let shows_metadata = metadata_width > 0 && lead_width + metadata_width <= width;
        let text_width = if shows_metadata {
            width - metadata_width
        } else {
            width
        };
        let mut spans = Vec::with_capacity(self.lead.len() + 2);
        spans.push(Span::styled(TREE_GUIDE.repeat(guides), self.guide_style));
        spans.extend(self.lead);
        self.description
            .push_spans(text_width.saturating_sub(lead_width), &mut spans);
        if shows_metadata {
            let used_width = spans_width(&spans);
            spans.push(Span::raw(" ".repeat(text_width - used_width)));
            spans.extend(metadata);
        }
        Line::from(spans)
    }
}

/// The metadata shown at the right edge of a row, in the fixed order recurrence, priority and
/// due date, each part separated by [`METADATA_GAP`].
///
/// Every part is shown in full or, if the row is too narrow, not at all; a task without the
/// metadata of a part has `None` there.
#[derive(Debug, Clone, PartialEq, Default)]
struct TrailingMetadata<'a> {
    /// The recurrence rule as stored, such as `every week`.
    recurrence: Option<MetadataPart<'a>>,
    /// The priority marker, such as `!!`.
    priority: Option<MetadataPart<'a>>,
    /// The due date relative to today, such as `in 3d`.
    due: Option<MetadataPart<'a>>,
}

impl<'a> TrailingMetadata<'a> {
    /// Returns the parts that are present in their order, each preceded by a gap; no spans
    /// at all if there is none.
    fn into_spans(self) -> Vec<Span<'a>> {
        [self.recurrence, self.priority, self.due]
            .into_iter()
            .flatten()
            .flat_map(|part| [Span::raw(METADATA_GAP), Span::styled(part.text, part.style)])
            .collect()
    }
}

/// Returns the cells `spans` take.
fn spans_width(spans: &[Span<'_>]) -> usize {
    spans.iter().map(|span| display_width(&span.content)).sum()
}

/// One part of the [`TrailingMetadata`] of a row.
#[derive(Debug, Clone, PartialEq)]
struct MetadataPart<'a> {
    text: Cow<'a, str>,
    style: Style,
}

/// Returns the recurrence part of the trailing metadata of `task`: its rule as stored, so that
/// a rule RuDo does not support stays visible.
fn recurrence_part<'a>(task: &'a TaskLine, theme: &Theme) -> Option<MetadataPart<'a>> {
    let rule = task.field(&FieldKey::Repeat)?.value();
    Some(MetadataPart {
        text: Cow::Borrowed(rule),
        style: theme.style(Slot::Recurrence),
    })
}

/// Returns the priority part of the trailing metadata of `task`: the marker of its priority,
/// or the value as written if it is no priority. Normal priority shows nothing.
fn priority_part<'a>(task: &'a TaskLine, theme: &Theme) -> Option<MetadataPart<'a>> {
    let value = task.field(&FieldKey::Priority)?.value();
    let (text, slot) = match value.parse::<Priority>() {
        Ok(priority) => priority_marker(priority),
        // An unknown value is shown as is, so that a typo stays visible.
        Err(_) => (value, Slot::Text),
    };
    Some(MetadataPart {
        text: Cow::Borrowed(text),
        style: theme.style(slot),
    })
}

/// Returns the due date part of the trailing metadata of `task`: its due date relative to
/// `today`, styled by how urgent it is.
///
/// A done or cancelled task is never overdue, so its past due date is shown as stored. A
/// stored value that is not a date is shown as is.
fn due_part<'a>(task: &'a TaskLine, theme: &Theme, today: Date) -> Option<MetadataPart<'a>> {
    let stored = task.field(&FieldKey::Due)?.value();
    let as_stored = MetadataPart {
        text: Cow::Borrowed(stored),
        style: theme.style(Slot::Due),
    };
    let Ok(due) = stored.parse::<Date>() else {
        return Some(as_stored);
    };
    Some(match DueClass::classify(due, today) {
        DueClass::Overdue(_) if is_closed(task.status()) => as_stored,
        class => MetadataPart {
            text: Cow::Owned(RelativeDue::new(due, today).to_string()),
            style: theme.style(due_slot(class)),
        },
    })
}

/// Returns the slot that styles a due date of `class`.
fn due_slot(class: DueClass) -> Slot {
    match class {
        DueClass::Overdue(_) => Slot::DueOverdue,
        DueClass::Today => Slot::DueToday,
        DueClass::Tomorrow | DueClass::Soon(_) => Slot::DueSoon,
        DueClass::Later => Slot::Due,
    }
}

/// Returns whether a task with `status` needs no more work.
fn is_closed(status: TaskStatus) -> bool {
    match status {
        TaskStatus::Done | TaskStatus::Cancelled => true,
        TaskStatus::Todo | TaskStatus::InProgress | TaskStatus::Other(_) => false,
    }
}

/// Returns the marker of `priority` and the slot that styles it.
fn priority_marker(priority: Priority) -> (&'static str, Slot) {
    match priority {
        Priority::Highest => ("!!!", Slot::PriorityHighest),
        Priority::High => ("!!", Slot::PriorityHigh),
        Priority::Medium => ("!", Slot::PriorityMedium),
        Priority::Low => ("-", Slot::PriorityLow),
        Priority::Lowest => ("--", Slot::PriorityLowest),
    }
}

/// The description of a task, styled by its status with its tags highlighted.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Description<'a> {
    text: &'a str,
    style: Style,
    tag_style: Style,
}

impl<'a> Description<'a> {
    /// Appends the description cut to `width` cells to `spans`.
    ///
    /// The text is cut first and the tags of the whole text are styled where they remain
    /// visible, so a tag that is cut keeps its style up to the ellipsis.
    fn push_spans(self, width: usize, spans: &mut Vec<Span<'a>>) {
        let (shown, ellipsis) = fitted(self.text, width);
        let mut position = 0;
        for tag in tags::spans(self.text).take_while(|tag| tag.start < shown.len()) {
            let end = tag.end.min(shown.len());
            spans.push(Span::styled(&shown[position..tag.start], self.style));
            spans.push(Span::styled(&shown[tag.start..end], self.tag_style));
            position = end;
        }
        spans.push(Span::styled(&shown[position..], self.style));
        spans.push(Span::styled(ellipsis, self.style));
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
    use jiff::civil::date;
    use ratatui::style::{Color, Modifier};
    use rstest::rstest;
    use strum::IntoEnumIterator;

    use super::*;
    use crate::app::tasks::task_items;
    use crate::config::keymap::keymap::KeyBindings;
    use crate::markdown::document::TodoDocument;
    use crate::ui::outline::{Outcome, OutlineCommand};

    const HEIGHT: usize = 10;
    /// A Tuesday.
    const TODAY: Date = date(2026, 11, 24);

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
        TaskRow::new(&rows[row], &Theme::default(), TODAY)
            .into_line(width)
            .to_string()
    }

    #[rstest]
    #[case::leaf("- [ ] a\n", &[], 0, "  [ ] a")]
    #[case::expanded("- [ ] a\n\t- [ ] b\n", &[], 0, "▾ [ ] a")]
    #[case::collapsed("- [ ] a\n\t- [ ] b\n", &[OutlineCommand::Collapse], 0, "▸ [ ] a")]
    #[case::subtask("- [ ] a\n\t- [x] b\n", &[], 1, "│   [x] b")]
    #[case::inline_fields_hidden("- [ ] a [completion:: 2026-03-20] #tag\n", &[], 0, "  [ ] a #tag")]
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

    #[rstest]
    #[case::highest("- [ ] a [priority:: highest]\n", 20, "  [ ] a          !!!")]
    #[case::high("- [ ] a [priority:: high]\n", 20, "  [ ] a           !!")]
    #[case::medium("- [ ] a [priority:: medium]\n", 20, "  [ ] a            !")]
    #[case::normal("- [ ] a\n", 20, "  [ ] a")]
    #[case::low("- [ ] a [priority:: low]\n", 20, "  [ ] a            -")]
    #[case::lowest("- [ ] a [priority:: lowest]\n", 20, "  [ ] a           --")]
    #[case::unknown_shown_as_is("- [ ] a [priority:: urgent]\n", 20, "  [ ] a       urgent")]
    #[case::description_cut_before_it("- [ ] abcdef [priority:: highest]\n", 14, "  [ ] abc… !!!")]
    #[case::only_the_lead_and_it("- [ ] a [priority:: highest]\n", 10, "  [ ]  !!!")]
    #[case::dropped_if_too_narrow("- [ ] a [priority:: highest]\n", 9, "  [ ] a")]
    fn priority_is_shown_at_the_right_edge(
        #[case] document: &str,
        #[case] width: usize,
        #[case] expected: &str,
    ) {
        let system_under_test = row_text(document, &[], 0, width);

        assert_eq!(system_under_test, expected);
    }

    #[rstest]
    #[case::alone("- [ ] a [repeat:: every week]\n", 20, "  [ ] a   every week")]
    #[case::unsupported_as_stored(
        "- [ ] a [repeat:: every weekday]\n",
        21,
        "  [ ] a every weekday"
    )]
    #[case::before_priority_and_due(
        "- [ ] a [due:: 2026-11-25] [priority:: high] [repeat:: every day]\n",
        29,
        "  [ ] a every day !! tomorrow"
    )]
    fn recurrence_comes_first_in_the_metadata(
        #[case] document: &str,
        #[case] width: usize,
        #[case] expected: &str,
    ) {
        let system_under_test = row_text(document, &[], 0, width);

        assert_eq!(system_under_test, expected);
    }

    #[rstest]
    #[case::highest(Priority::Highest, ("!!!", Slot::PriorityHighest))]
    #[case::high(Priority::High, ("!!", Slot::PriorityHigh))]
    #[case::medium(Priority::Medium, ("!", Slot::PriorityMedium))]
    #[case::low(Priority::Low, ("-", Slot::PriorityLow))]
    #[case::lowest(Priority::Lowest, ("--", Slot::PriorityLowest))]
    fn priority_has_its_marker_and_slot(
        #[case] priority: Priority,
        #[case] expected: (&str, Slot),
    ) {
        let system_under_test = priority_marker(priority);

        assert_eq!(system_under_test, expected);
    }

    #[test]
    fn priority_marker_is_styled_with_its_slot() {
        let document = TodoDocument::parse("- [ ] a [priority:: high]\n");
        let theme = Theme::default();
        let expected = Some(MetadataPart {
            text: Cow::Borrowed("!!"),
            style: theme.style(Slot::PriorityHigh),
        });

        let system_under_test = priority_part(document.tasks().next().unwrap().task(), &theme);

        assert_eq!(system_under_test, expected);
    }

    #[rstest]
    #[case::overdue("- [ ] a [due:: 2026-11-22]\n", "  [ ] a         2d overdue")]
    #[case::today("- [/] a [due:: 2026-11-24]\n", "  [/] a              today")]
    #[case::later("- [ ] a [due:: 2026-12-24]\n", "  [ ] a             Dec 24")]
    #[case::done_is_not_overdue("- [x] a [due:: 2026-11-22]\n", "  [x] a         2026-11-22")]
    #[case::cancelled_is_not_overdue("- [-] a [due:: 2026-11-22]\n", "  [-] a         2026-11-22")]
    #[case::done_due_later("- [x] a [due:: 2026-11-25]\n", "  [x] a           tomorrow")]
    #[case::unparseable("- [ ] a [due:: someday]\n", "  [ ] a            someday")]
    fn row_shows_the_due_date_at_the_right_end(#[case] document: &str, #[case] expected: &str) {
        let system_under_test = row_text(document, &[], 0, 26);

        assert_eq!(system_under_test, expected);
    }

    #[rstest]
    #[case::description_fits(26, "  [ ] abcdef    2d overdue")]
    #[case::description_cut(19, "  [ ] a… 2d overdue")]
    #[case::description_gone(17, "  [ ]  2d overdue")]
    #[case::metadata_dropped(16, "  [ ] abcdef")]
    fn narrow_rows_cut_the_description_before_dropping_the_metadata(
        #[case] width: usize,
        #[case] expected: &str,
    ) {
        let document = "- [ ] abcdef [due:: 2026-11-22]\n";

        let system_under_test = row_text(document, &[], 0, width);

        assert_eq!(system_under_test, expected);
    }

    #[rstest]
    #[case::overdue(DueClass::Overdue(1), Slot::DueOverdue)]
    #[case::today(DueClass::Today, Slot::DueToday)]
    #[case::tomorrow(DueClass::Tomorrow, Slot::DueSoon)]
    #[case::soon(DueClass::Soon(7), Slot::DueSoon)]
    #[case::later(DueClass::Later, Slot::Due)]
    fn due_class_picks_its_theme_slot(#[case] class: DueClass, #[case] expected: Slot) {
        let system_under_test = due_slot(class);

        assert_eq!(system_under_test, expected);
    }

    #[rstest]
    #[case::todo(TaskStatus::Todo, false)]
    #[case::in_progress(TaskStatus::InProgress, false)]
    #[case::other(TaskStatus::Other('?'), false)]
    #[case::done(TaskStatus::Done, true)]
    #[case::cancelled(TaskStatus::Cancelled, true)]
    fn only_done_and_cancelled_tasks_are_closed(
        #[case] status: TaskStatus,
        #[case] expected: bool,
    ) {
        let system_under_test = is_closed(status);

        assert_eq!(system_under_test, expected);
    }

    /// Returns the text and style of every non-empty span of the description of the only
    /// task of `document`, fitted into `width` cells.
    fn description_spans(document: &str, width: usize) -> Vec<(String, Style)> {
        let document = TodoDocument::parse(document);
        let items = task_items(&document);
        let mut outline = Outline::new();
        outline.refresh(&items, HEIGHT);
        let rows = outline.visible_rows(&items);
        let line = TaskRow::new(&rows[0], &Theme::default(), TODAY).into_line(width);
        line.spans[4..]
            .iter()
            .filter(|span| !span.content.is_empty())
            .map(|span| (span.content.to_string(), span.style))
            .collect()
    }

    const TAG: Style = Style::new().fg(Color::Magenta);
    const DONE: Style = Style::new().fg(Color::DarkGray);

    #[rstest]
    #[case::no_tag("- [ ] a b\n", 40, &[("a b", Style::new())])]
    #[case::tag_between_text(
        "- [ ] a #b, c\n",
        40,
        &[("a ", Style::new()), ("#b", TAG), (", c", Style::new())]
    )]
    #[case::status_styles_the_rest("- [x] #a b\n", 40, &[("#a", TAG), (" b", DONE)])]
    #[case::code_span_is_not_a_tag("- [ ] `#a`\n", 40, &[("`#a`", Style::new())])]
    #[case::cut_tag_keeps_its_style(
        "- [ ] a #long\n",
        11,
        &[("a ", Style::new()), ("#l", TAG), ("\u{2026}", Style::new())]
    )]
    #[case::cut_before_the_tag(
        "- [ ] abc #tag\n",
        9,
        &[("ab", Style::new()), ("\u{2026}", Style::new())]
    )]
    fn description_highlights_tags_after_cutting(
        #[case] document: &str,
        #[case] width: usize,
        #[case] expected: &[(&str, Style)],
    ) {
        let expected: Vec<(String, Style)> = expected
            .iter()
            .map(|&(text, style)| (text.to_owned(), style))
            .collect();

        let system_under_test = description_spans(document, width);

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

    /// Nested tasks: `a` with the subtask `b`, then `c`.
    const NESTED: &str = "- [ ] a\n\t- [ ] b\n- [ ] c\n";

    /// Returns the placeholder slot of a new task at `position` in [`NESTED`], given the
    /// description of the reference task.
    fn slot_in_nested(position: fn(TaskId) -> InsertPosition, reference: &str) -> (usize, usize) {
        let document = TodoDocument::parse(NESTED);
        let items = task_items(&document);
        let id = *items
            .iter()
            .find(|item| item.task().description() == reference)
            .unwrap()
            .key();
        let outline = Outline::new();
        placeholder_slot(&outline.visible_rows(&items), position(id))
    }

    #[rstest]
    #[case::below_a_block(InsertPosition::Below, "a", (2, 0))]
    #[case::below_a_subtask(InsertPosition::Below, "b", (2, 1))]
    #[case::above(InsertPosition::Above, "c", (2, 0))]
    #[case::subtask_after_the_last_subtask(InsertPosition::LastChild, "a", (2, 1))]
    #[case::first_subtask(InsertPosition::LastChild, "c", (3, 1))]
    #[case::end(|_| InsertPosition::End, "a", (3, 0))]
    fn placeholder_goes_where_the_task_will_be_inserted(
        #[case] position: fn(TaskId) -> InsertPosition,
        #[case] reference: &str,
        #[case] expected: (usize, usize),
    ) {
        let system_under_test = slot_in_nested(position, reference);

        assert_eq!(system_under_test, expected);
    }

    #[rstest]
    #[case::already_visible(2, 5, 10, 2)]
    #[case::above_the_viewport(4, 1, 10, 1)]
    #[case::below_the_viewport(0, 12, 10, 3)]
    #[case::without_rows(0, 3, 0, 4)]
    fn offset_showing_scrolls_just_enough_to_show_the_row(
        #[case] offset: usize,
        #[case] row: usize,
        #[case] height: usize,
        #[case] expected: usize,
    ) {
        let system_under_test = offset_showing(offset, row, height);

        assert_eq!(system_under_test, expected);
    }

    #[test]
    fn new_task_row_shows_guides_and_an_open_checkbox() {
        let expected = "│ │   [ ] ";

        let system_under_test = TaskRow::new_task(2, &Theme::default())
            .into_line(40)
            .to_string();

        assert_eq!(system_under_test, expected);
    }

    #[test]
    fn edited_task_row_keeps_only_marker_and_checkbox() {
        let document = TodoDocument::parse("- [x] a\n\t- [ ] b\n");
        let items = task_items(&document);
        let outline = Outline::new();
        let rows = outline.visible_rows(&items);
        let expected = "▾ [x] ";

        let system_under_test = TaskRow::new(&rows[0], &Theme::default(), TODAY)
            .lead_only()
            .into_line(40)
            .to_string();

        assert_eq!(system_under_test, expected);
    }
}

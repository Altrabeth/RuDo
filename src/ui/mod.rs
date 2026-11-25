//! Terminal user interface: pure view models such as the outline, and widgets that render
//! the application state with `ratatui`.
//!
//! [`view`] divides the terminal with [`layout::Screen`] and lets every region render itself:
//! [`lists_pane`] and [`tasks_pane`] inside their borders and [`status_bar`] below them.
//! Overlays such as [`help_overlay`], the [`popup`] of an open prompt or dialog and the
//! [`picker`] are drawn last, over the panes. All styles come from [`App::theme`].

pub mod help;
pub mod help_overlay;
pub(crate) mod highlight;
pub mod layout;
pub mod lists_pane;
pub mod outline;
pub(crate) mod pane_message;
pub mod picker;
pub mod popup;
pub mod status_bar;
pub mod tasks_pane;
pub mod text_input;

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::widgets::{Paragraph, Wrap};

use crate::app::{App, Focus, Mode};
use crate::config::theme::Slot;
use crate::ui::layout::{PaneBorder, PaneSwitchHint, Screen, render_pane_border};
use crate::ui::picker::PickerPopup;
use crate::ui::popup::{ConfirmDialog, InputPopup};

/// Shown instead of the panes if the terminal is too small for them.
const TOO_SMALL: &str = "terminal too small";

/// Renders `app` into `frame`; the runtime calls it after every batch of events.
///
/// Wide terminals show the lists pane and the tasks pane side by side, narrow ones only the
/// focused pane, and tiny ones only a message; see [`Screen::new`].
pub fn view(app: &App, frame: &mut Frame) {
    match Screen::new(frame.area(), app.focus()) {
        Screen::TooSmall(area) => render_too_small(app, frame, area),
        Screen::SideBySide {
            lists,
            tasks,
            bottom_bar,
        } => {
            render_pane(app, frame, Focus::Lists, lists, PaneSwitchHint::Hidden);
            render_pane(app, frame, Focus::Tasks, tasks, PaneSwitchHint::Hidden);
            status_bar::render(app, frame, bottom_bar);
        }
        Screen::FocusedOnly {
            pane,
            area,
            bottom_bar,
        } => {
            render_pane(app, frame, pane, area, PaneSwitchHint::Shown);
            status_bar::render(app, frame, bottom_bar);
        }
    }
    help_overlay::render(app, frame);
    render_popup(app, frame);
}

/// Draws the input popup of [`Mode::Prompt`], the picker of [`Mode::FolderPicker`] or the
/// dialog of [`Mode::Confirm`] over the rest of the interface.
fn render_popup(app: &App, frame: &mut Frame) {
    let area = frame.area();
    match app.mode() {
        Mode::Prompt(prompt) => {
            InputPopup::new(prompt.prompt(), app.theme()).render_with_cursor(frame, area);
        }
        Mode::FolderPicker(folder_picker) => {
            PickerPopup::new(folder_picker.picker(), folder_picker.title(), app.theme())
                .hint(folder_picker.hint())
                .error(folder_picker.error())
                .render_with_cursor(frame, area);
        }
        Mode::Confirm(confirm) => {
            let dialog = ConfirmDialog::new(confirm.message(), app.keymap(), app.theme());
            frame.render_widget(dialog, area);
        }
        Mode::Normal | Mode::Help(_) | Mode::InlineEdit(_) | Mode::Search(_) | Mode::Visual => {}
    }
}

/// Draws the border of `pane` around `area` and its contents inside.
fn render_pane(app: &App, frame: &mut Frame, pane: Focus, area: Rect, hint: PaneSwitchHint) {
    let title = match pane {
        Focus::Lists => lists_pane::title(app),
        Focus::Tasks => tasks_pane::title(app),
    };
    let border = PaneBorder {
        pane,
        focus: app.focus(),
        title: &title,
        hint,
    };
    let inner = render_pane_border(frame, area, border, app.theme(), app.keymap());
    match pane {
        Focus::Lists => lists_pane::render(app, frame, inner),
        Focus::Tasks => tasks_pane::render(app, frame, inner),
    }
}

/// Renders the [`TOO_SMALL`] message, centred and starting in the middle row of `area`.
fn render_too_small(app: &App, frame: &mut Frame, area: Rect) {
    let [_, lower_half] = Layout::vertical([Constraint::Fill(1); 2]).areas(area);
    let message = Paragraph::new(TOO_SMALL)
        .style(app.theme().style(Slot::Text))
        .centered()
        .wrap(Wrap { trim: true });
    frame.render_widget(message, lower_half);
}

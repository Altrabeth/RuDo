//! Terminal user interface: pure view models such as the outline, and widgets that render
//! the application state with `ratatui`.
//!
//! [`view`] divides the terminal with [`layout::Screen`] and lets every region render itself:
//! [`lists_pane`] and [`tasks_pane`] inside their borders and [`status_bar`] below them.
//! Overlays such as [`help_overlay`] and the [`popup`] of an open prompt are drawn last, over
//! the panes. All styles come from [`App::theme`].

pub mod help;
pub mod help_overlay;
pub mod layout;
pub mod lists_pane;
pub mod outline;
pub(crate) mod pane_message;
pub mod popup;
pub mod status_bar;
pub mod tasks_pane;
pub mod text_input;

use std::borrow::Cow;

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::widgets::{Paragraph, Wrap};

use crate::app::{App, Focus, Mode};
use crate::config::theme::Slot;
use crate::ui::layout::{PaneBorder, PaneSwitchHint, Screen, render_pane_border};
use crate::ui::popup::InputPopup;

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
    render_prompt(app, frame);
}

/// Draws the input popup over the rest of the interface if `app` is in [`Mode::Prompt`].
fn render_prompt(app: &App, frame: &mut Frame) {
    match app.mode() {
        Mode::Prompt(prompt) => {
            let area = frame.area();
            InputPopup::new(prompt.prompt(), app.theme()).render_with_cursor(frame, area);
        }
        Mode::Normal | Mode::Help(_) => {}
    }
}

/// Draws the border of `pane` around `area` and its contents inside.
fn render_pane(app: &App, frame: &mut Frame, pane: Focus, area: Rect, hint: PaneSwitchHint) {
    let title = match pane {
        Focus::Lists => Cow::Borrowed(lists_pane::TITLE),
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

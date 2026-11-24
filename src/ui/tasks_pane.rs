//! The tasks pane: the task tree of the open list.

use ratatui::Frame;
use ratatui::layout::Rect;

use crate::app::App;

/// Title of the tasks pane.
pub const TITLE: &str = "Tasks";

/// Renders the contents of the tasks pane of `app` into `area`, the inside of its border.
///
/// The pane is still empty; the task tree follows.
pub fn render(_app: &App, _frame: &mut Frame, _area: Rect) {}

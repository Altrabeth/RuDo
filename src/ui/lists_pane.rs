//! The lists pane: the workspace tree of folders and lists.

use ratatui::Frame;
use ratatui::layout::Rect;

use crate::app::App;

/// Title of the lists pane.
pub const TITLE: &str = "Lists";

/// Renders the contents of the lists pane of `app` into `area`, the inside of its border.
///
/// The pane is still empty; the workspace tree follows.
pub fn render(_app: &App, _frame: &mut Frame, _area: Rect) {}

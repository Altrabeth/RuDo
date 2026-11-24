//! The bottom bar: key hints, pending keys and messages.

use ratatui::Frame;
use ratatui::layout::Rect;

use crate::app::App;

/// Renders the bottom bar of `app` into `area`, a single line below the panes.
///
/// The bar is still empty; the key hints and messages follow.
pub fn render(_app: &App, _frame: &mut Frame, _area: Rect) {}

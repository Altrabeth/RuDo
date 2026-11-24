//! Terminal user interface: pure view models such as the outline, and widgets that render
//! the application state with `ratatui`.

pub mod help;
pub mod outline;
pub mod text_input;

use ratatui::Frame;
use ratatui::widgets::Block;

use crate::app::App;

/// Title shown in the border around the interface.
const TITLE: &str = "RuDo";

/// Renders `app` into `frame`; the runtime calls it after every batch of events.
///
/// For now this is only a bordered block with the title; the panes follow.
pub fn view(_app: &App, frame: &mut Frame) {
    frame.render_widget(Block::bordered().title(TITLE), frame.area());
}

#[cfg(test)]
mod tests {
    use jiff::civil::date;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::buffer::Buffer;
    use tempfile::TempDir;

    use super::*;
    use crate::config::Config;
    use crate::config::keymap::keymap::Keymap;
    use crate::storage::workspace::Workspace;

    #[test]
    fn view_draws_a_titled_border() {
        let directory = TempDir::new().unwrap();
        let config = Config::default();
        let keymap = Keymap::new(config.keybindings()).unwrap();
        let workspace = Workspace::scan(directory.path(), config.tasks().indent()).unwrap();
        let app = App::new(workspace, config, keymap, date(2026, 3, 14));
        let mut terminal = Terminal::new(TestBackend::new(8, 3)).unwrap();
        let expected = Buffer::with_lines(["┌RuDo──┐", "│      │", "└──────┘"]);

        let system_under_test = terminal.draw(|frame| view(&app, frame)).unwrap();

        assert_eq!(*system_under_test.buffer, expected);
    }
}

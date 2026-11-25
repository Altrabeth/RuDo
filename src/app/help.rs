//! The state of the help screen in [`Mode::Help`]: the rows it shows, where it is scrolled
//! to and the filter typed into it.
//!
//! The rows come from the effective keymap through [`help_sections`]; scrolling is an
//! [`Outline`] over their [`HelpRowKey`]s, so the help screen moves exactly like the panes.

use ratatui::layout::{Rect, Size};

use crate::app::state::{App, Focus, Mode};
use crate::config::keymap::action::Context;
use crate::config::keymap::key::KeyChord;
use crate::config::keymap::keymap::Keymap;
use crate::ui::help::{HelpHeading, HelpRow, HelpRowKey, HelpSection, filter, help_sections, rows};
use crate::ui::layout::{FilterLine, help_overlay};
use crate::ui::outline::{Outcome, Outline, OutlineCommand};
use crate::ui::text_input::{TextChange, TextInput};

/// The filter of the help screen.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
pub enum HelpFilter {
    /// No filter: every row is shown.
    #[default]
    Off,
    /// The filter input is open and takes the typed keys; the rows follow it live.
    Editing(TextInput),
    /// The typed filter was accepted; the keys navigate the filtered rows again.
    Applied(TextInput),
}

impl HelpFilter {
    /// Returns whether the overlay shows the filter in its last row.
    #[must_use]
    pub const fn line(&self) -> FilterLine {
        match self {
            Self::Off => FilterLine::Hidden,
            Self::Editing(_) | Self::Applied(_) => FilterLine::Shown,
        }
    }
}

/// The help screen: the sections matching the filter, the scroll state and the filter.
#[derive(Debug, Clone)]
pub struct HelpOverlay {
    sections: Vec<HelpSection>,
    outline: Outline<HelpRowKey>,
    filter: HelpFilter,
}

impl HelpOverlay {
    /// Opens the help screen for `keymap` on a terminal of `screen_size`, scrolled to the
    /// section of the pane that has the `focus`.
    fn open(keymap: &Keymap, section: HelpHeading, screen_size: Size) -> Self {
        let mut help = Self {
            sections: help_sections(keymap),
            outline: Outline::new(),
            filter: HelpFilter::Off,
        };
        let row_keys = help.row_keys();
        let viewport_height = help.viewport_height(screen_size);
        let section = HelpRowKey::Heading(section);
        // Coming from the bottom, selecting the heading scrolls up just far enough to show
        // it, so the section starts near the top of the overlay instead of near its bottom.
        let _: Outcome = help
            .outline
            .apply(&row_keys, OutlineCommand::Bottom, viewport_height);
        let _: Outcome = help.outline.select(&row_keys, &section, viewport_height);
        help
    }

    /// Returns the sections shown, reduced to the entries matching the filter.
    #[must_use]
    pub fn sections(&self) -> &[HelpSection] {
        &self.sections
    }

    /// Returns the index of the row shown at the top of the overlay.
    #[must_use]
    pub fn offset(&self) -> usize {
        self.outline.offset()
    }

    /// Returns the selected row, or `None` if the filter matches nothing.
    #[must_use]
    pub fn selected(&self) -> Option<HelpRowKey> {
        self.outline.selected().copied()
    }

    /// Returns the filter.
    #[must_use]
    pub fn filter(&self) -> &HelpFilter {
        &self.filter
    }

    /// Returns the context in which key presses are resolved: the filter input while it is
    /// open, the help screen otherwise.
    pub(super) const fn context(&self) -> Context {
        match self.filter {
            HelpFilter::Editing(_) => Context::Input,
            HelpFilter::Off | HelpFilter::Applied(_) => Context::Help,
        }
    }

    /// Scrolls with `command`.
    pub(super) fn navigate(&mut self, command: OutlineCommand, screen_size: Size) {
        let row_keys = self.row_keys();
        let viewport_height = self.viewport_height(screen_size);
        // Navigating past the first or last row has no further meaning on the help screen.
        let _: Outcome = self.outline.apply(&row_keys, command, viewport_height);
    }

    /// Keeps the selection inside the viewport after the terminal or the filter row changed.
    pub(super) fn refresh(&mut self, screen_size: Size) {
        let row_keys = self.row_keys();
        self.outline
            .refresh(&row_keys, self.viewport_height(screen_size));
    }

    /// Opens the filter input, continuing an applied filter.
    pub(super) fn open_filter(&mut self, screen_size: Size) {
        self.filter = match std::mem::take(&mut self.filter) {
            HelpFilter::Off => HelpFilter::Editing(TextInput::new()),
            HelpFilter::Editing(input) | HelpFilter::Applied(input) => HelpFilter::Editing(input),
        };
        self.refresh(screen_size);
    }

    /// Closes the filter input and keeps the filter, unless it is empty.
    pub(super) fn submit_filter(&mut self, screen_size: Size) {
        self.filter = match std::mem::take(&mut self.filter) {
            HelpFilter::Editing(input) if input.value().is_empty() => HelpFilter::Off,
            HelpFilter::Editing(input) | HelpFilter::Applied(input) => HelpFilter::Applied(input),
            HelpFilter::Off => HelpFilter::Off,
        };
        self.refresh(screen_size);
    }

    /// Closes the filter input and clears the filter, showing every row again.
    pub(super) fn cancel_filter(&mut self, keymap: &Keymap, screen_size: Size) {
        match self.filter {
            HelpFilter::Editing(_) => {
                self.filter = HelpFilter::Off;
                self.show_sections(help_sections(keymap), screen_size);
            }
            HelpFilter::Off | HelpFilter::Applied(_) => {}
        }
    }

    /// Edits the open filter input with a key press that no action is bound to.
    pub(super) fn press(&mut self, chord: KeyChord, keymap: &Keymap, screen_size: Size) {
        let HelpFilter::Editing(input) = &mut self.filter else {
            return;
        };
        match input.press(chord) {
            TextChange::Changed => {
                let sections = filter(&help_sections(keymap), input.value());
                self.show_sections(sections, screen_size);
            }
            TextChange::Unchanged => {}
        }
    }

    /// Inserts pasted text into the open filter input.
    pub(super) fn paste(&mut self, text: &str, keymap: &Keymap, screen_size: Size) {
        let HelpFilter::Editing(input) = &mut self.filter else {
            return;
        };
        input.insert_str(text);
        let sections = filter(&help_sections(keymap), input.value());
        self.show_sections(sections, screen_size);
    }

    /// Shows `sections` from their first row, because the rows above the selection changed.
    fn show_sections(&mut self, sections: Vec<HelpSection>, screen_size: Size) {
        self.sections = sections;
        self.outline = Outline::new();
        self.refresh(screen_size);
    }

    fn row_keys(&self) -> Vec<HelpRowKey> {
        rows(&self.sections).map(HelpRow::key).collect()
    }

    fn viewport_height(&self, screen_size: Size) -> usize {
        let screen = Rect::from(screen_size);
        usize::from(help_overlay(screen, self.filter.line()).rows.height)
    }
}

/// Returns the help section that explains the pane with the `focus`.
const fn section_of(focus: Focus) -> HelpHeading {
    match focus {
        Focus::Lists => HelpHeading::ListsPane,
        Focus::Tasks => HelpHeading::TasksPane,
    }
}

impl App {
    /// Opens the help screen over the focused pane and discards a pending key sequence.
    pub(super) fn open_help(&mut self) {
        self.resolver.reset();
        let section = match self.lists.bin_view() {
            Some(_) => HelpHeading::Bin,
            None => section_of(self.focus),
        };
        self.mode = Mode::Help(HelpOverlay::open(&self.keymap, section, self.screen_size));
    }

    /// Closes the help screen.
    pub(super) fn close_help(&mut self) {
        match self.mode {
            Mode::Help(_) => self.mode = Mode::Normal,
            Mode::Normal
            | Mode::Prompt(_)
            | Mode::FolderPicker(_)
            | Mode::GoTo(_)
            | Mode::Confirm(_)
            | Mode::InlineEdit(_)
            | Mode::Search(_)
            | Mode::Visual => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use tempfile::TempDir;

    use super::*;
    use crate::app::Effect;
    use crate::app::state::tests::app_in;
    use crate::config::keymap::action::Action;
    use crate::config::keymap::keymap::KeyBindings;
    use crate::config::keymap::resolver::Resolution;

    const SCREEN: Size = Size::new(80, 24);

    /// Starts an app on an 80x24 terminal and performs `actions`, none of which has effects.
    fn app_after(root: &Path, actions: &[Action]) -> App {
        let mut app = app_in(root);
        app.resize(SCREEN);
        for action in actions {
            assert_eq!(app.update(*action), []);
        }
        app
    }

    fn help_of(app: &App) -> &HelpOverlay {
        match app.mode() {
            Mode::Help(help) => help,
            Mode::Normal
            | Mode::Prompt(_)
            | Mode::FolderPicker(_)
            | Mode::GoTo(_)
            | Mode::Confirm(_)
            | Mode::InlineEdit(_)
            | Mode::Search(_)
            | Mode::Visual => {
                panic!("the help screen is not open")
            }
        }
    }

    #[test]
    fn opening_the_help_discards_a_pending_key_sequence() {
        let directory = TempDir::new().unwrap();
        let mut app = app_after(directory.path(), &[]);
        let _: Resolution = app
            .resolver
            .feed(&app.keymap, Context::Lists, "g".parse().unwrap());

        let system_under_test = app.update(Action::Help);

        assert_eq!(system_under_test, []);
        assert_eq!(app.resolver.pending(), []);
    }

    #[test]
    fn close_help_without_the_help_keeps_normal_mode() {
        let directory = TempDir::new().unwrap();
        let mut app = app_after(directory.path(), &[]);

        let system_under_test = app.update(Action::CloseHelp);

        assert_eq!(system_under_test, []);
        assert!(matches!(app.mode(), Mode::Normal));
    }

    #[test]
    fn opened_help_shows_every_section_without_filter() {
        let keymap = Keymap::new(&KeyBindings::default()).unwrap();
        let expected = help_sections(&keymap);

        let system_under_test = HelpOverlay::open(&keymap, HelpHeading::TasksPane, SCREEN);

        assert_eq!(system_under_test.sections(), expected);
        assert_eq!(system_under_test.filter(), &HelpFilter::Off);
        assert_eq!(system_under_test.context(), Context::Help);
    }

    #[test]
    fn submitting_an_empty_filter_turns_it_off() {
        let directory = TempDir::new().unwrap();
        let mut app = app_after(directory.path(), &[Action::Help, Action::Search]);

        let system_under_test = app.update(Action::Submit);

        assert_eq!(system_under_test, []);
        assert_eq!(help_of(&app).filter(), &HelpFilter::Off);
    }

    #[test]
    fn search_continues_an_applied_filter() {
        let directory = TempDir::new().unwrap();
        let mut app = app_after(directory.path(), &[Action::Help, Action::Search]);
        assert_eq!(app.handle_paste("due"), []);
        assert_eq!(app.update(Action::Submit), []);
        let expected = HelpFilter::Editing(TextInput::with_value("due"));

        let system_under_test = app.update(Action::Search);

        assert_eq!(system_under_test, []);
        assert_eq!(help_of(&app).filter(), &expected);
    }

    #[test]
    fn filter_actions_without_open_input_change_nothing() {
        let directory = TempDir::new().unwrap();
        let mut app = app_after(directory.path(), &[Action::Help]);
        let expected_offset = help_of(&app).offset();

        let system_under_test: Vec<Effect> = [Action::Submit, Action::Cancel]
            .into_iter()
            .flat_map(|action| app.update(action))
            .collect();

        assert_eq!(system_under_test, []);
        assert_eq!(help_of(&app).filter(), &HelpFilter::Off);
        assert_eq!(help_of(&app).offset(), expected_offset);
    }

    #[test]
    fn pastes_outside_the_filter_input_are_ignored() {
        let directory = TempDir::new().unwrap();
        let mut app = app_after(directory.path(), &[Action::Help]);
        let expected_sections = help_of(&app).sections().to_vec();

        let system_under_test = app.handle_paste("due");

        assert_eq!(system_under_test, []);
        assert_eq!(help_of(&app).sections(), expected_sections);
    }

    #[test]
    fn resizing_keeps_the_last_row_visible() {
        let directory = TempDir::new().unwrap();
        let small_screen = Size::new(80, 10);
        let mut app = app_after(directory.path(), &[Action::Help, Action::MoveToBottom]);
        app.resize(small_screen);
        let row_count = rows(help_of(&app).sections()).count();
        let expected_offset = row_count - help_of(&app).viewport_height(small_screen);

        let system_under_test = help_of(&app);

        assert_eq!(system_under_test.offset(), expected_offset);
    }
}

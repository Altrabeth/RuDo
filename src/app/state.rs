//! Everything the application knows: [`App`] with its [`Focus`] and [`Mode`].

use jiff::civil::Date;
use ratatui::layout::{Rect, Size};

use crate::app::help::HelpOverlay;
use crate::app::lists::ListsPane;
use crate::app::prompt::PromptOverlay;
use crate::app::status::Status;
use crate::app::tasks::TasksPane;
use crate::config::Config;
use crate::config::keymap::action::Context;
use crate::config::keymap::key::KeyChord;
use crate::config::keymap::keymap::Keymap;
use crate::config::keymap::resolver::KeyResolver;
use crate::config::theme::Theme;
use crate::storage::workspace::Workspace;
use crate::ui::layout::pane_content;

/// The complete state of a running RuDo.
///
/// [`App::handle_key`] and [`App::update`] decide what happens and return the
/// [`Effect`](crate::app::Effect)s to perform; [`execute`](crate::app::execute) performs them
/// and reports their results back through methods such as [`App::save_failed`]. Documents are
/// only ever changed through [`App::edit_list`].
#[derive(Debug)]
pub struct App {
    pub(super) workspace: Workspace,
    pub(super) config: Config,
    pub(super) theme: Theme,
    pub(super) keymap: Keymap,
    pub(super) resolver: KeyResolver,
    pub(super) focus: Focus,
    pub(super) lists: ListsPane,
    pub(super) tasks: TasksPane,
    pub(super) mode: Mode,
    pub(super) today: Date,
    pub(super) status: Status,
    pub(super) should_quit: bool,
    /// Size of the terminal, from which scrolling commands derive the viewport heights.
    pub(super) screen_size: Size,
}

impl App {
    /// Starts the application on `workspace` with the lists pane focused and the first list
    /// selected and opened.
    ///
    /// `theme` and `keymap` are the effective theme and keymap built from `config`, as
    /// [`Settings::into_parts`](crate::config::settings::Settings::into_parts) returns them,
    /// and `today` is the current date, which the runtime updates with [`App::set_today`].
    #[must_use]
    pub fn new(
        workspace: Workspace,
        config: Config,
        theme: Theme,
        keymap: Keymap,
        today: Date,
    ) -> Self {
        // The size is unknown until the runtime calls `resize`, which scrolls the panes again.
        let screen_size = Size::ZERO;
        let mut app = Self {
            lists: ListsPane::new(&workspace, viewport_height(screen_size, Focus::Lists)),
            tasks: TasksPane::default(),
            workspace,
            config,
            theme,
            keymap,
            resolver: KeyResolver::default(),
            focus: Focus::Lists,
            mode: Mode::Normal,
            today,
            status: Status::default(),
            should_quit: false,
            screen_size,
        };
        app.refresh_tasks();
        app
    }

    /// Returns the workspace with its loaded lists.
    #[must_use]
    pub fn workspace(&self) -> &Workspace {
        &self.workspace
    }

    /// Returns the configuration.
    #[must_use]
    pub fn config(&self) -> &Config {
        &self.config
    }

    /// Returns the effective theme, from which every widget takes its styles.
    #[must_use]
    pub fn theme(&self) -> &Theme {
        &self.theme
    }

    /// Returns the effective keymap.
    #[must_use]
    pub fn keymap(&self) -> &Keymap {
        &self.keymap
    }

    /// Returns the keys typed so far of an unfinished key sequence, empty if none is pending.
    #[must_use]
    pub fn pending_keys(&self) -> &[KeyChord] {
        self.resolver.pending()
    }

    /// Returns the focused pane.
    #[must_use]
    pub fn focus(&self) -> Focus {
        self.focus
    }

    /// Returns the current mode.
    #[must_use]
    pub fn mode(&self) -> &Mode {
        &self.mode
    }

    /// Returns the [`Context`] in which key presses are resolved, derived from the mode and
    /// the focus.
    #[must_use]
    pub fn context(&self) -> Context {
        match &self.mode {
            Mode::Normal => self.focus.context(),
            Mode::Help(help) => help.context(),
            Mode::Prompt(_) => Context::Input,
        }
    }

    /// Returns the size of the terminal as last reported by [`App::resize`].
    #[must_use]
    pub fn screen_size(&self) -> Size {
        self.screen_size
    }

    /// Sets the size of the terminal; the runtime calls it at start-up and whenever the
    /// terminal is resized, because scrolling depends on the height of the panes.
    ///
    /// Every pane and overlay scrolls again so that its selection stays visible.
    pub fn resize(&mut self, screen_size: Size) {
        self.screen_size = screen_size;
        self.lists
            .refresh(&self.workspace, self.viewport_height(Focus::Lists));
        self.refresh_tasks();
        match &mut self.mode {
            Mode::Normal | Mode::Prompt(_) => {}
            Mode::Help(help) => help.refresh(screen_size),
        }
    }

    /// Returns the number of rows `pane` shows on the current terminal, by which it scrolls.
    pub(super) fn viewport_height(&self, pane: Focus) -> usize {
        viewport_height(self.screen_size, pane)
    }

    /// Returns the date the application treats as today.
    #[must_use]
    pub fn today(&self) -> Date {
        self.today
    }

    /// Sets the date the application treats as today; the runtime calls it when the date
    /// changes, because the core never reads the clock.
    pub fn set_today(&mut self, today: Date) {
        self.today = today;
    }

    /// Returns whether the user asked to quit; the runtime stops when it is set.
    #[must_use]
    pub fn should_quit(&self) -> bool {
        self.should_quit
    }
}

/// Returns the number of rows `pane` shows on a terminal of `screen_size`, as the view draws it.
fn viewport_height(screen_size: Size, pane: Focus) -> usize {
    usize::from(pane_content(Rect::from(screen_size), pane).height)
}

/// The pane that receives key presses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, strum::EnumIter)]
pub enum Focus {
    /// The lists pane with the workspace tree.
    Lists,
    /// The tasks pane with the tasks of the open list.
    Tasks,
}

impl Focus {
    /// Returns the pane after this one, wrapping around.
    pub(super) const fn next(self) -> Self {
        match self {
            Self::Lists => Self::Tasks,
            Self::Tasks => Self::Lists,
        }
    }

    /// Returns the pane before this one, wrapping around.
    pub(super) const fn previous(self) -> Self {
        match self {
            Self::Lists => Self::Tasks,
            Self::Tasks => Self::Lists,
        }
    }

    const fn context(self) -> Context {
        match self {
            Self::Lists => Context::Lists,
            Self::Tasks => Context::Tasks,
        }
    }
}

/// What key presses mean apart from the focused pane: plain navigation and commands, or an
/// overlay such as the help screen or a text input that takes them first.
#[derive(Debug, Clone, Default)]
#[non_exhaustive]
pub enum Mode {
    /// Keys trigger the actions bound in the focused pane.
    #[default]
    Normal,
    /// The help screen covers the panes and takes the keys.
    Help(HelpOverlay),
    /// An input popup asks for text, such as the name of a new list, and takes the keys.
    Prompt(PromptOverlay),
}

#[cfg(test)]
pub(super) mod tests {
    use std::path::Path;

    use jiff::civil::date;
    use rstest::rstest;
    use strum::IntoEnumIterator;
    use tempfile::TempDir;

    use super::*;
    use crate::markdown::document::{EditOutcome, TodoDocument};
    use crate::storage::workspace::{Entry, ListPath};

    /// Starts an app with the default configuration on the workspace in `root`.
    pub(in crate::app) fn app_in(root: &Path) -> App {
        let config = Config::default();
        let keymap = Keymap::new(config.keybindings()).unwrap();
        let workspace = Workspace::scan(root, config.tasks().indent()).unwrap();
        App::new(
            workspace,
            config,
            Theme::default(),
            keymap,
            date(2026, 3, 14),
        )
    }

    /// Returns the path of the only list in the workspace of `app`.
    pub(in crate::app) fn only_list_path(app: &App) -> ListPath {
        match app.workspace().entries() {
            [Entry::List(list)] => list.path().clone(),
            entries => panic!("expected one list, got {entries:?}"),
        }
    }

    /// Marks the first task of `document` as done, or as todo again.
    pub(in crate::app) fn toggle_first_task(document: &mut TodoDocument) -> EditOutcome {
        let id = document.tasks().next().unwrap().id();
        document.toggle_done(id, None)
    }

    #[test]
    fn new_app_starts_in_the_lists_pane_in_normal_mode() {
        let directory = TempDir::new().unwrap();

        let system_under_test = app_in(directory.path());

        assert_eq!(system_under_test.focus(), Focus::Lists);
        assert!(matches!(system_under_test.mode(), Mode::Normal));
        assert_eq!(system_under_test.screen_size(), Size::ZERO);
        assert_eq!(system_under_test.status_message(), None);
        assert!(!system_under_test.should_quit());
        assert_eq!(system_under_test.workspace().root(), directory.path());
        assert_eq!(system_under_test.config(), &Config::default());
        assert_eq!(system_under_test.theme(), &Theme::default());
    }

    #[rstest]
    #[case::lists(Focus::Lists, Context::Lists)]
    #[case::tasks(Focus::Tasks, Context::Tasks)]
    fn normal_mode_resolves_keys_in_the_focused_pane(
        #[case] focus: Focus,
        #[case] expected: Context,
    ) {
        let directory = TempDir::new().unwrap();
        let mut app = app_in(directory.path());
        app.focus = focus;

        let system_under_test = app.context();

        assert_eq!(system_under_test, expected);
    }

    #[test]
    fn set_today_replaces_the_date() {
        let directory = TempDir::new().unwrap();
        let mut app = app_in(directory.path());
        let tomorrow = date(2026, 3, 15);
        app.set_today(tomorrow);

        let system_under_test = app.today();

        assert_eq!(system_under_test, tomorrow);
    }

    #[test]
    fn resize_remembers_the_screen_size() {
        let directory = TempDir::new().unwrap();
        let mut app = app_in(directory.path());
        let screen_size = Size::new(80, 24);
        app.resize(screen_size);

        let system_under_test = app.screen_size();

        assert_eq!(system_under_test, screen_size);
    }

    #[rstest]
    #[case::lists(Focus::Lists, Focus::Tasks)]
    #[case::tasks(Focus::Tasks, Focus::Lists)]
    fn next_wraps_around(#[case] focus: Focus, #[case] expected: Focus) {
        let system_under_test = focus.next();

        assert_eq!(system_under_test, expected);
    }

    #[rstest]
    #[case::lists(Focus::Lists, Focus::Tasks)]
    #[case::tasks(Focus::Tasks, Focus::Lists)]
    fn previous_wraps_around(#[case] focus: Focus, #[case] expected: Focus) {
        let system_under_test = focus.previous();

        assert_eq!(system_under_test, expected);
    }

    #[test]
    fn previous_undoes_next() {
        let panes: Vec<Focus> = Focus::iter().collect();

        let system_under_test: Vec<Focus> =
            Focus::iter().map(|pane| pane.next().previous()).collect();

        assert_eq!(system_under_test, panes);
    }
}

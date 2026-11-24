//! The responsive layout: where the panes, the bottom bar and the overlays go for a terminal
//! size, and the border that shows which pane has the focus.
//!
//! The functions here are the single source of the geometry. The view draws into the areas
//! they return, and the app derives the viewport heights for scrolling from the same areas
//! ([`pane_content`], [`help_overlay`]), so navigation always moves by the height that is
//! drawn.

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Margin, Rect};
use ratatui::text::Line;
use ratatui::widgets::{Block, BorderType};

use crate::app::Focus;
use crate::config::keymap::action::Action;
use crate::config::keymap::keymap::Keymap;
use crate::config::theme::{Slot, Theme};
use crate::ui::status_bar::key_hint;

/// Below this width, or [`MIN_HEIGHT`], only a short message is shown.
const MIN_WIDTH: u16 = 24;
/// Two border rows, one content row and the bottom bar.
const MIN_HEIGHT: u16 = 5;
/// From this width on both panes are shown side by side.
const SIDE_BY_SIDE_MIN_WIDTH: u16 = 60;
/// Share of the width taken by the lists pane, within [`LISTS_MIN_WIDTH`] and
/// [`LISTS_MAX_WIDTH`].
const LISTS_WIDTH_PERCENT: u32 = 30;
const LISTS_MIN_WIDTH: u16 = 24;
const LISTS_MAX_WIDTH: u16 = 40;
const BOTTOM_BAR_HEIGHT: u16 = 1;
/// The border around a pane or an overlay takes one cell on every side.
const BORDER: Margin = Margin::new(1, 1);
/// Share of the terminal width and height covered by the help overlay.
const HELP_OVERLAY_PERCENT: u16 = 90;

/// The actions whose keys a lone pane shows in its title, because the other pane is hidden.
const PANE_SWITCH_ACTIONS: [Action; 2] = [Action::FocusLists, Action::FocusTasks];

/// How the terminal area is divided, computed by [`Screen::new`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Screen {
    /// The terminal is too small for any pane; a short message fills the area.
    TooSmall(Rect),
    /// The lists pane on the left, the tasks pane on the right and the bottom bar below.
    SideBySide {
        /// Area of the lists pane, including its border.
        lists: Rect,
        /// Area of the tasks pane, including its border.
        tasks: Rect,
        /// Area of the bottom bar.
        bottom_bar: Rect,
    },
    /// Only the focused pane, over the full width, and the bottom bar below.
    FocusedOnly {
        /// The pane that is shown.
        pane: Focus,
        /// Area of the pane, including its border.
        area: Rect,
        /// Area of the bottom bar.
        bottom_bar: Rect,
    },
}

impl Screen {
    /// Divides `area` for a terminal of its size in which `focus` has the focus.
    #[must_use]
    pub fn new(area: Rect, focus: Focus) -> Self {
        if area.width < MIN_WIDTH || area.height < MIN_HEIGHT {
            return Self::TooSmall(area);
        }
        let [panes, bottom_bar] =
            Layout::vertical([Constraint::Fill(1), Constraint::Length(BOTTOM_BAR_HEIGHT)])
                .areas(area);
        if area.width < SIDE_BY_SIDE_MIN_WIDTH {
            return Self::FocusedOnly {
                pane: focus,
                area: panes,
                bottom_bar,
            };
        }
        let [lists, tasks] = Layout::horizontal([
            Constraint::Length(lists_width(area.width)),
            Constraint::Fill(1),
        ])
        .areas(panes);
        Self::SideBySide {
            lists,
            tasks,
            bottom_bar,
        }
    }
}

/// Returns the area inside the border of `pane` on a terminal of the size of `screen`, into
/// which the pane renders its rows; empty if the terminal is too small for any pane.
///
/// A pane has the focus whenever it is navigated, so the area is the one `pane` has while it
/// is focused. Its height is the same whichever pane has the focus.
#[must_use]
pub fn pane_content(screen: Rect, pane: Focus) -> Rect {
    let area = match Screen::new(screen, pane) {
        Screen::TooSmall(_) => return Rect::ZERO,
        Screen::SideBySide { lists, tasks, .. } => match pane {
            Focus::Lists => lists,
            Focus::Tasks => tasks,
        },
        Screen::FocusedOnly { area, .. } => area,
    };
    inside_border(area)
}

/// Returns the area inside the border drawn around `area`.
fn inside_border(area: Rect) -> Rect {
    area.inner(BORDER)
}

/// Whether the help overlay reserves its last row for the filter.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FilterLine {
    /// No filter is open or applied; every row shows help rows.
    Hidden,
    /// The last row shows the filter.
    Shown,
}

/// Where the parts of the help overlay go, computed by [`help_overlay`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct HelpLayout {
    /// The overlay including its border.
    pub overlay: Rect,
    /// The rows of the help screen inside the border.
    pub rows: Rect,
    /// The row of the filter, if it is shown.
    pub filter: Option<Rect>,
}

/// Divides the help overlay for a terminal of the size of `screen`.
///
/// The overlay covers 90 % of the terminal in both directions, centred.
#[must_use]
pub fn help_overlay(screen: Rect, filter_line: FilterLine) -> HelpLayout {
    let overlay = screen.centered(
        Constraint::Percentage(HELP_OVERLAY_PERCENT),
        Constraint::Percentage(HELP_OVERLAY_PERCENT),
    );
    let inside = inside_border(overlay);
    match filter_line {
        FilterLine::Hidden => HelpLayout {
            overlay,
            rows: inside,
            filter: None,
        },
        FilterLine::Shown => {
            let [rows, filter] =
                Layout::vertical([Constraint::Fill(1), Constraint::Length(1)]).areas(inside);
            HelpLayout {
                overlay,
                rows,
                filter: Some(filter),
            }
        }
    }
}

/// Returns the width of the lists pane in a terminal `terminal_width` columns wide.
fn lists_width(terminal_width: u16) -> u16 {
    let share = u32::from(terminal_width) * LISTS_WIDTH_PERCENT / 100;
    u16::try_from(share)
        .unwrap_or(u16::MAX)
        .clamp(LISTS_MIN_WIDTH, LISTS_MAX_WIDTH)
}

/// Whether a pane title also shows the keys that switch panes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PaneSwitchHint {
    /// Both panes are visible, so no hint is needed.
    Hidden,
    /// The other pane is hidden; the title tells how to reach it.
    Shown,
}

/// The border of one pane: what it shows and whether it has the focus.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PaneBorder<'a> {
    /// The pane the border belongs to.
    pub pane: Focus,
    /// The pane that has the focus.
    pub focus: Focus,
    /// The title in the top left corner.
    pub title: &'a str,
    /// Whether the keys that switch panes are shown in the top right corner.
    pub hint: PaneSwitchHint,
}

/// Draws `border` around `area` and returns the area inside it, into which the pane renders
/// its contents; it is the area [`pane_content`] returns for the same pane.
///
/// The border and title use the `border_focused` and `title_focused` slots of `theme` if the
/// pane has the focus, and `border` and `title` otherwise.
pub fn render_pane_border(
    frame: &mut Frame,
    area: Rect,
    border: PaneBorder<'_>,
    theme: &Theme,
    keymap: &Keymap,
) -> Rect {
    let (border_slot, title_slot) = if border.pane == border.focus {
        (Slot::BorderFocused, Slot::TitleFocused)
    } else {
        (Slot::Border, Slot::Title)
    };
    let mut block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(theme.style(border_slot))
        .title_style(theme.style(title_slot))
        .title(Line::from_iter([" ", border.title, " "]));
    match border.hint {
        PaneSwitchHint::Hidden => {}
        PaneSwitchHint::Shown => {
            block = block.title(pane_switch_hint(theme, keymap).right_aligned());
        }
    }
    frame.render_widget(block, area);
    inside_border(area)
}

/// Returns the keys that switch panes with their labels, such as ` 1 lists 2 tasks `.
fn pane_switch_hint(theme: &Theme, keymap: &Keymap) -> Line<'static> {
    Line::from_iter(
        PANE_SWITCH_ACTIONS
            .into_iter()
            .filter_map(|action| key_hint(keymap, action, theme))
            .flatten(),
    )
}

#[cfg(test)]
mod tests {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use rstest::rstest;

    use super::*;
    use crate::config::keymap::keymap::KeyBindings;

    fn keymap(yaml: &str) -> Keymap {
        let overrides: KeyBindings = if yaml.is_empty() {
            KeyBindings::default()
        } else {
            serde_saphyr::from_str(yaml).unwrap()
        };
        Keymap::new(&overrides).unwrap()
    }

    fn inner_area(area: Rect, border: PaneBorder<'_>) -> Rect {
        let keymap = keymap("");
        let mut terminal = Terminal::new(TestBackend::new(40, 20)).unwrap();
        let mut inner = Rect::default();
        terminal
            .draw(|frame| {
                inner = render_pane_border(frame, area, border, &Theme::default(), &keymap);
            })
            .unwrap();
        inner
    }

    fn top_row(width: u16, border: PaneBorder<'_>, keymap: &Keymap) -> String {
        let mut terminal = Terminal::new(TestBackend::new(width, 3)).unwrap();
        terminal
            .draw(|frame| {
                render_pane_border(frame, frame.area(), border, &Theme::default(), keymap);
            })
            .unwrap();
        let buffer = terminal.backend().buffer();
        (0..width).map(|x| buffer[(x, 0)].symbol()).collect()
    }

    #[rstest]
    #[case::too_narrow(23, 24)]
    #[case::too_low(80, 4)]
    #[case::empty(0, 0)]
    fn small_terminals_are_too_small(#[case] width: u16, #[case] height: u16) {
        let area = Rect::new(0, 0, width, height);

        let system_under_test = Screen::new(area, Focus::Lists);

        assert_eq!(system_under_test, Screen::TooSmall(area));
    }

    #[rstest]
    #[case::lists(Focus::Lists)]
    #[case::tasks(Focus::Tasks)]
    fn narrow_terminals_show_only_the_focused_pane(#[case] focus: Focus) {
        let area = Rect::new(0, 0, 59, 5);
        let expected = Screen::FocusedOnly {
            pane: focus,
            area: Rect::new(0, 0, 59, 4),
            bottom_bar: Rect::new(0, 4, 59, 1),
        };

        let system_under_test = Screen::new(area, focus);

        assert_eq!(system_under_test, expected);
    }

    #[rstest]
    #[case::minimum(60, 24)]
    #[case::thirty_percent(100, 30)]
    #[case::maximum(134, 40)]
    #[case::wide(1000, 40)]
    #[case::widest(u16::MAX, 40)]
    fn wide_terminals_show_both_panes(#[case] width: u16, #[case] expected_lists_width: u16) {
        let area = Rect::new(0, 0, width, 24);
        let expected = Screen::SideBySide {
            lists: Rect::new(0, 0, expected_lists_width, 23),
            tasks: Rect::new(expected_lists_width, 0, width - expected_lists_width, 23),
            bottom_bar: Rect::new(0, 23, width, 1),
        };

        let system_under_test = Screen::new(area, Focus::Tasks);

        assert_eq!(system_under_test, expected);
    }

    #[rstest]
    #[case::lists_side_by_side(Rect::new(0, 0, 80, 24), Focus::Lists, Rect::new(1, 1, 22, 21))]
    #[case::tasks_side_by_side(Rect::new(0, 0, 80, 40), Focus::Tasks, Rect::new(25, 1, 54, 37))]
    #[case::lists_alone(Rect::new(0, 0, 40, 12), Focus::Lists, Rect::new(1, 1, 38, 9))]
    #[case::tasks_alone(Rect::new(0, 0, 40, 12), Focus::Tasks, Rect::new(1, 1, 38, 9))]
    #[case::too_small(Rect::new(0, 0, 20, 4), Focus::Lists, Rect::ZERO)]
    fn pane_content_is_the_area_inside_the_border(
        #[case] screen: Rect,
        #[case] pane: Focus,
        #[case] expected: Rect,
    ) {
        let system_under_test = pane_content(screen, pane);

        assert_eq!(system_under_test, expected);
    }

    #[rstest]
    #[case::hidden(
        FilterLine::Hidden,
        HelpLayout {
            overlay: Rect::new(4, 1, 72, 22),
            rows: Rect::new(5, 2, 70, 20),
            filter: None,
        },
    )]
    #[case::shown(
        FilterLine::Shown,
        HelpLayout {
            overlay: Rect::new(4, 1, 72, 22),
            rows: Rect::new(5, 2, 70, 19),
            filter: Some(Rect::new(5, 21, 70, 1)),
        },
    )]
    fn help_overlay_is_centred(#[case] filter_line: FilterLine, #[case] expected: HelpLayout) {
        let screen = Rect::new(0, 0, 80, 24);

        let system_under_test = help_overlay(screen, filter_line);

        assert_eq!(system_under_test, expected);
    }

    #[rstest]
    #[case::empty(Rect::ZERO)]
    #[case::one_row(Rect::new(0, 0, 3, 1))]
    fn help_overlay_of_a_tiny_screen_has_no_rows(#[case] screen: Rect) {
        let system_under_test = help_overlay(screen, FilterLine::Shown);

        assert_eq!(system_under_test.rows.height, 0);
    }

    #[test]
    fn border_returns_the_area_inside_it() {
        let area = Rect::new(2, 3, 30, 10);
        let border = PaneBorder {
            pane: Focus::Lists,
            focus: Focus::Lists,
            title: "Lists",
            hint: PaneSwitchHint::Hidden,
        };
        let expected = Rect::new(3, 4, 28, 8);

        let system_under_test = inner_area(area, border);

        assert_eq!(system_under_test, expected);
    }

    #[rstest]
    #[case::hidden(PaneSwitchHint::Hidden, "", "╭ Lists ─────────────────────────────╮")]
    #[case::shown(PaneSwitchHint::Shown, "", "╭ Lists ─────────── 1 lists  2 tasks ╮")]
    #[case::rebound(
        PaneSwitchHint::Shown,
        "focus-lists: [L, ctrl-l]",
        "╭ Lists ─────────── L lists  2 tasks ╮"
    )]
    #[case::unbound(
        PaneSwitchHint::Shown,
        "focus-lists: []",
        "╭ Lists ────────────────────── 2 tasks ╮"
    )]
    fn title_shows_the_pane_switch_keys_only_when_asked(
        #[case] hint: PaneSwitchHint,
        #[case] keybindings: &str,
        #[case] expected: &str,
    ) {
        let border = PaneBorder {
            pane: Focus::Lists,
            focus: Focus::Lists,
            title: "Lists",
            hint,
        };
        let keymap = keymap(keybindings);
        let width = u16::try_from(expected.chars().count()).unwrap();

        let system_under_test = top_row(width, border, &keymap);

        assert_eq!(system_under_test, expected);
    }
}

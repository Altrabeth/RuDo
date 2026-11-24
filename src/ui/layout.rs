//! The responsive layout: where the panes and the bottom bar go for a terminal size, and the
//! border that shows which pane has the focus.

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
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
/// its contents.
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
    let inner = block.inner(area);
    frame.render_widget(block, area);
    inner
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

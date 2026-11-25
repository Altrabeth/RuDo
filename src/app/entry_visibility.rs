//! Showing and hiding hidden folders and lists in the lists pane with `toggle-hidden`.
//!
//! The workspace always holds the hidden entries, those whose name or folder starts with `.`;
//! the lists pane leaves them out of the items its [`Outline`](crate::ui::outline::Outline)
//! gets, through [`prune`](crate::ui::outline::prune), as the tasks pane does with completed
//! tasks.

use crate::app::state::{App, Focus};
use crate::storage::workspace::Entry;

/// Whether the lists pane shows hidden folders and lists.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, strum::EnumIter)]
pub enum HiddenEntries {
    /// Hidden folders and lists are shown, dimmed.
    Shown,
    /// Hidden folders and lists are left out together with everything inside them.
    #[default]
    Hidden,
}

impl HiddenEntries {
    /// Returns the visibility that the setting `workspace.show_hidden` asks for.
    #[must_use]
    pub const fn from_setting(show_hidden: bool) -> Self {
        if show_hidden {
            Self::Shown
        } else {
            Self::Hidden
        }
    }

    /// Returns the other visibility.
    #[must_use]
    pub const fn toggled(self) -> Self {
        match self {
            Self::Shown => Self::Hidden,
            Self::Hidden => Self::Shown,
        }
    }

    /// Tells whether `entry` is shown.
    #[must_use]
    pub fn shows(self, entry: &Entry) -> bool {
        match self {
            Self::Shown => true,
            Self::Hidden => !entry.is_hidden(),
        }
    }
}

impl App {
    /// Returns whether the lists pane shows hidden folders and lists.
    #[must_use]
    pub fn hidden_entries(&self) -> HiddenEntries {
        self.lists.hidden_entries()
    }

    /// Shows hidden folders and lists if they are hidden and hides them otherwise.
    ///
    /// A selected entry that becomes hidden hands the selection to the nearest visible entry.
    pub(super) fn toggle_hidden(&mut self) {
        let viewport_height = self.viewport_height(Focus::Lists);
        self.lists.toggle_hidden(&self.workspace, viewport_height);
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::Path;

    use rstest::rstest;
    use strum::IntoEnumIterator;
    use tempfile::TempDir;

    use super::*;
    use crate::config::IndentStyle;
    use crate::storage::workspace::Workspace;

    #[rstest]
    #[case::shown(true, HiddenEntries::Shown)]
    #[case::hidden(false, HiddenEntries::Hidden)]
    fn setting_decides_the_initial_visibility(
        #[case] show_hidden: bool,
        #[case] expected: HiddenEntries,
    ) {
        let system_under_test = HiddenEntries::from_setting(show_hidden);

        assert_eq!(system_under_test, expected);
    }

    #[test]
    fn toggling_twice_restores_every_visibility() {
        let expected: Vec<HiddenEntries> = HiddenEntries::iter().collect();

        let system_under_test: Vec<HiddenEntries> = HiddenEntries::iter()
            .map(|visibility| visibility.toggled().toggled())
            .collect();

        assert_eq!(system_under_test, expected);
        assert!(HiddenEntries::iter().all(|visibility| visibility.toggled() != visibility));
    }

    #[rstest]
    #[case::visible_shown(HiddenEntries::Shown, "inbox.md", true)]
    #[case::visible_hidden(HiddenEntries::Hidden, "inbox.md", true)]
    #[case::hidden_shown(HiddenEntries::Shown, ".notes", true)]
    #[case::hidden_hidden(HiddenEntries::Hidden, ".notes", false)]
    #[case::inside_hidden_hidden(HiddenEntries::Hidden, ".notes/ideas.md", false)]
    fn shows_hidden_entries_only_while_shown(
        #[case] visibility: HiddenEntries,
        #[case] path: &str,
        #[case] expected: bool,
    ) {
        let root = TempDir::new().unwrap();
        fs::create_dir(root.path().join(".notes")).unwrap();
        fs::write(root.path().join(".notes/ideas.md"), "").unwrap();
        fs::write(root.path().join("inbox.md"), "").unwrap();
        let workspace = Workspace::scan(root.path(), IndentStyle::default()).unwrap();
        let entry = workspace.entry(Path::new(path)).unwrap();

        let system_under_test = visibility.shows(entry);

        assert_eq!(system_under_test, expected);
    }
}

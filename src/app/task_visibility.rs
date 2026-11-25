//! Showing and hiding completed tasks in the tasks pane with `toggle-completed`.
//!
//! Hidden tasks are left out of the items the [`Outline`](crate::ui::outline::Outline) gets,
//! through [`prune`](crate::ui::outline::prune), so an open subtask of a completed task is
//! hidden with its parent.

use crate::app::state::App;
use crate::markdown::task_line::TaskStatus;

/// Whether the tasks pane shows done and cancelled tasks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, strum::EnumIter)]
pub enum CompletedTasks {
    /// Done and cancelled tasks are shown.
    #[default]
    Shown,
    /// Done and cancelled tasks are hidden together with their subtasks.
    Hidden,
}

impl CompletedTasks {
    /// Returns the visibility that the setting `tasks.show_completed` asks for.
    #[must_use]
    pub const fn from_setting(show_completed: bool) -> Self {
        if show_completed {
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

    /// Tells whether a task with `status` is shown.
    #[must_use]
    pub const fn shows(self, status: TaskStatus) -> bool {
        match self {
            Self::Shown => true,
            Self::Hidden => !status.is_completed(),
        }
    }
}

impl App {
    /// Returns whether the tasks pane shows completed tasks.
    #[must_use]
    pub fn completed_tasks(&self) -> CompletedTasks {
        self.completed_tasks
    }

    /// Shows completed tasks if they are hidden and hides them otherwise.
    ///
    /// A selected task that becomes hidden hands the selection to the next visible task.
    pub(super) fn toggle_completed(&mut self) {
        self.completed_tasks = self.completed_tasks.toggled();
        self.refresh_tasks();
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;
    use strum::IntoEnumIterator;

    use super::*;

    #[rstest]
    #[case::shown(true, CompletedTasks::Shown)]
    #[case::hidden(false, CompletedTasks::Hidden)]
    fn setting_decides_the_initial_visibility(
        #[case] show_completed: bool,
        #[case] expected: CompletedTasks,
    ) {
        let system_under_test = CompletedTasks::from_setting(show_completed);

        assert_eq!(system_under_test, expected);
    }

    #[test]
    fn toggling_twice_restores_every_visibility() {
        let expected: Vec<CompletedTasks> = CompletedTasks::iter().collect();

        let system_under_test: Vec<CompletedTasks> = CompletedTasks::iter()
            .map(|visibility| visibility.toggled().toggled())
            .collect();

        assert_eq!(system_under_test, expected);
        assert!(CompletedTasks::iter().all(|visibility| visibility.toggled() != visibility));
    }

    #[test]
    fn shown_completed_tasks_show_every_status() {
        let visibility = CompletedTasks::Shown;

        let system_under_test = TaskStatus::iter().all(|status| visibility.shows(status));

        assert!(system_under_test);
    }

    #[rstest]
    #[case::todo(TaskStatus::Todo, true)]
    #[case::in_progress(TaskStatus::InProgress, true)]
    #[case::other(TaskStatus::Other('?'), true)]
    #[case::done(TaskStatus::Done, false)]
    #[case::cancelled(TaskStatus::Cancelled, false)]
    fn hidden_completed_tasks_show_only_open_statuses(
        #[case] status: TaskStatus,
        #[case] expected: bool,
    ) {
        let visibility = CompletedTasks::Hidden;

        let system_under_test = visibility.shows(status);

        assert_eq!(system_under_test, expected);
    }
}

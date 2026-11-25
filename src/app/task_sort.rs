//! Sorting the selected task and its siblings with `s p`, `s d`, `s a` and `s s`.

use crate::app::effect::Effect;
use crate::app::state::App;
use crate::markdown::document::SortKey;

impl App {
    /// Sorts the selected task of the active list and its siblings by `key`, keeping the task
    /// selected.
    pub(super) fn sort_tasks(&mut self, key: SortKey) -> Vec<Effect> {
        self.restructure_selected_task(|document, task| document.sort_siblings(task, key))
    }
}

#[cfg(test)]
mod tests {
    use strum::IntoEnumIterator;
    use tempfile::TempDir;

    use super::*;
    use crate::app::state::tests::app_in;

    #[test]
    fn sorting_without_an_active_list_has_no_effect() {
        let directory = TempDir::new().unwrap();
        let mut app = app_in(directory.path());

        let system_under_test: Vec<Vec<Effect>> =
            SortKey::iter().map(|key| app.sort_tasks(key)).collect();

        assert!(system_under_test.iter().all(Vec::is_empty));
    }
}

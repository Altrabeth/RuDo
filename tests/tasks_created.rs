//! The creation date `[created:: YYYY-MM-DD]` that RuDo writes on every task it creates.

#[expect(
    dead_code,
    reason = "each test crate uses only part of the shared harness"
)]
mod common;

use std::path::Path;

use rstest::rstest;
use rudo::app::Mode;
use rudo::app::tasks::visible_task_items;
use rudo::config::Config;
use rudo::storage::watch::{FsChange, PathState};

use crate::common::TestApp;

/// A task created on another day, with its creation date between the other fields.
const PLANTS: &str = "- [ ] Water  [created:: 2026-01-05]  plants [due:: 2026-03-20]\n";

/// Starts the app on `Plants.md` with `content` and focuses the tasks pane.
fn plants(content: &str) -> TestApp {
    let mut test_app = TestApp::new(&[("Plants.md", content)]);
    test_app.press("2");
    test_app
}

/// Returns the descriptions of the tasks the tasks pane shows, in display order.
fn shown_tasks(test_app: &TestApp) -> Vec<String> {
    let app = test_app.app();
    let loaded = app.active_list().unwrap().content().unwrap();
    visible_task_items(loaded.document(), app.completed_tasks(), app.task_filter())
        .iter()
        .map(|item| item.task().description().to_owned())
        .collect()
}

#[rstest]
#[case::below("o")]
#[case::above("O")]
#[case::subtask("a")]
fn new_task_gets_todays_creation_date(#[case] key: &str) {
    let expected = "- [ ] Feed [created:: 2026-03-14]";
    let mut test_app = plants("- [ ] Water\n");
    test_app.press(key).type_text("Feed").press("enter");

    let system_under_test = test_app.read("Plants.md");

    assert!(system_under_test.contains(expected), "{system_under_test}");
}

#[test]
fn without_created_date_a_new_task_has_none() {
    let config =
        Config::from_yaml("tasks:\n  created_date: false\n", Path::new("config.yaml")).unwrap();
    let expected = "- [ ] Water\n- [ ] Feed\n";
    let mut test_app = TestApp::with_config(&[("Plants.md", "- [ ] Water\n")], config);
    test_app.press("2 o").type_text("Feed").press("enter");

    let system_under_test = test_app.read("Plants.md");

    assert_eq!(system_under_test, expected);
}

#[test]
fn typed_creation_date_is_kept() {
    let expected = "- [ ] Water\n- [ ] Feed [created:: 2026-02-01]\n";
    let mut test_app = plants("- [ ] Water\n");
    test_app
        .press("o")
        .type_text("Feed [created:: 2026-02-01]")
        .press("enter");

    let system_under_test = test_app.read("Plants.md");

    assert_eq!(system_under_test, expected);
}

#[rstest]
#[case::undo("u", "- [ ] Water\n")]
#[case::redo("u ctrl-r", "- [ ] Water\n- [ ] Feed [created:: 2026-03-14]\n")]
fn undo_and_redo_take_the_creation_date_with_the_task(#[case] keys: &str, #[case] expected: &str) {
    let mut test_app = plants("- [ ] Water\n");
    test_app.press("o").type_text("Feed").press("enter");

    let system_under_test = test_app.press(keys);

    assert_eq!(system_under_test.read("Plants.md"), expected);
}

#[rstest]
#[case::without_a_creation_date("- [ ] Water\n", "- [ ] Water\n- [ ] Water\n")]
#[case::with_a_creation_date(PLANTS, &format!("{PLANTS}{PLANTS}"))]
fn pasted_task_keeps_its_fields_and_gets_no_creation_date(
    #[case] content: &str,
    #[case] expected: &str,
) {
    let mut test_app = plants(content);

    let system_under_test = test_app.press("y y p");

    assert_eq!(system_under_test.read("Plants.md"), expected);
}

#[rstest]
#[case::other_task_edited(
    &format!("- [ ] Feed\n{PLANTS}"),
    "x",
    &format!("- [x] Feed [completion:: 2026-03-14]\n{PLANTS}")
)]
#[case::other_field_of_the_task_edited(
    PLANTS,
    "+",
    "- [ ] Water plants [created:: 2026-01-05] [due:: 2026-03-20] [priority:: medium]\n"
)]
#[case::description_edited(
    PLANTS,
    "i ! enter",
    "- [ ] Water plants! [created:: 2026-01-05] [due:: 2026-03-20]\n"
)]
fn existing_creation_date_keeps_its_place(
    #[case] content: &str,
    #[case] keys: &str,
    #[case] expected: &str,
) {
    let mut test_app = plants(content);

    let system_under_test = test_app.press(keys);

    assert_eq!(system_under_test.read("Plants.md"), expected);
}

#[test]
fn creation_date_survives_a_reload() {
    let expected = Some(jiff::civil::date(2026, 1, 5));
    let mut test_app = plants("- [ ] Water\n");
    test_app.write("Plants.md", PLANTS);
    let changes = [FsChange::new("Plants.md", PathState::File)];
    test_app.run(|app| app.on_fs_changed(&changes));

    let system_under_test = test_app.app().active_list().unwrap().content().unwrap();

    assert_eq!(
        system_under_test
            .document()
            .tasks()
            .next()
            .unwrap()
            .task()
            .created(),
        expected
    );
    assert_eq!(system_under_test.document().to_string(), PLANTS);
}

#[test]
fn search_does_not_match_the_creation_date() {
    let mut test_app = plants(&format!("{PLANTS}- [ ] Feed 2026-01-05\n"));
    let expected = ["Feed 2026-01-05"];
    test_app.press("/").type_text("2026-01").press("enter");

    let system_under_test = shown_tasks(&test_app);

    assert_eq!(system_under_test, expected);
}

#[test]
fn go_to_does_not_match_the_creation_date() {
    let mut test_app = plants(PLANTS);
    test_app.press("ctrl-p").type_text("created");

    let system_under_test = match test_app.app().mode() {
        Mode::GoTo(prompt) => prompt.picker().results(0).count(),
        mode => panic!("go to anything is not open, the mode is {mode:?}"),
    };

    assert_eq!(system_under_test, 0);
}

#[test]
fn row_does_not_show_the_creation_date() {
    let mut test_app = plants(PLANTS);

    let system_under_test = test_app.render(80, 6).to_string();

    assert!(
        !system_under_test.contains("created"),
        "{system_under_test}"
    );
    assert!(
        !system_under_test.contains("2026-01-05"),
        "{system_under_test}"
    );
}

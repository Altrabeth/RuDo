//! User scenarios of the application core: focus, quitting and saving edits.

#[expect(
    dead_code,
    reason = "each test crate uses only part of the shared harness"
)]
mod common;

use rstest::rstest;
use rudo::app::{Focus, Severity};
use rudo::markdown::document::{EditOutcome, TodoDocument};

use crate::common::TestApp;

fn toggle_first_task(document: &mut TodoDocument) -> EditOutcome {
    let id = document.tasks().next().unwrap().id();
    document.toggle_done(id, None)
}

#[rstest]
#[case::start("", Focus::Lists)]
#[case::next("tab", Focus::Tasks)]
#[case::next_wraps_around("tab tab", Focus::Lists)]
#[case::previous_wraps_around("backtab", Focus::Tasks)]
#[case::previous("backtab backtab", Focus::Lists)]
#[case::tasks("2", Focus::Tasks)]
#[case::tasks_again("2 2", Focus::Tasks)]
#[case::lists("2 1", Focus::Lists)]
fn focus_follows_the_focus_keys(#[case] keys: &str, #[case] expected: Focus) {
    let mut test_app = TestApp::new(&[]);

    let system_under_test = test_app.press(keys);

    assert_eq!(system_under_test.app().focus(), expected);
}

#[rstest]
#[case::q("q")]
#[case::ctrl_c("ctrl-c")]
#[case::from_the_tasks_pane("2 q")]
fn quit_keys_quit(#[case] keys: &str) {
    let mut test_app = TestApp::new(&[]);

    let system_under_test = test_app.press(keys);

    assert!(system_under_test.app().should_quit());
}

#[test]
fn app_does_not_quit_on_its_own() {
    let system_under_test = TestApp::new(&[]);

    assert!(!system_under_test.app().should_quit());
}

#[test]
fn typed_characters_are_handled_as_key_presses() {
    let mut test_app = TestApp::new(&[]);

    let system_under_test = test_app.type_text("2");

    assert_eq!(system_under_test.app().focus(), Focus::Tasks);
}

#[rstest]
#[case::top_level("groceries.md")]
#[case::in_a_folder("home/groceries.md")]
fn edit_list_saves_the_change(#[case] path: &str) {
    let mut test_app = TestApp::new(&[(path, "# Groceries\n- [ ] Buy milk\n")]);
    let list = test_app.list_path(path);
    let expected = "# Groceries\n- [x] Buy milk\n";

    let system_under_test = test_app.run(|app| app.edit_list(&list, toggle_first_task));

    assert_eq!(system_under_test.read(path), expected);
    assert_eq!(system_under_test.app().status_message(), None);
}

#[test]
fn consecutive_edits_are_all_saved() {
    let original = "- [ ] Buy milk\n";
    let mut test_app = TestApp::new(&[("groceries.md", original)]);
    let list = test_app.list_path("groceries.md");
    test_app.run(|app| app.edit_list(&list, toggle_first_task));

    let system_under_test = test_app.run(|app| app.edit_list(&list, toggle_first_task));

    assert_eq!(system_under_test.read("groceries.md"), original);
    assert_eq!(system_under_test.app().status_message(), None);
}

#[test]
fn edit_of_an_externally_modified_list_is_reported_and_not_saved() {
    let external = "- [ ] Buy bread\n";
    let mut test_app = TestApp::new(&[("groceries.md", "- [ ] Buy milk\n")]);
    let list = test_app.list_path("groceries.md");
    test_app.write("groceries.md", external);

    let system_under_test = test_app.run(|app| app.edit_list(&list, toggle_first_task));

    assert_eq!(system_under_test.read("groceries.md"), external);
    assert!(
        system_under_test
            .app()
            .status_message()
            .is_some_and(|message| {
                message.severity() == Severity::Error
                    && message.text()
                        == "groceries changed on disk; your last change was not applied"
            })
    );
}

//! Sorting the lists pane by name, urgency, priority, open tasks and total tasks: a view of
//! the workspace that never writes to disk.

#[expect(
    dead_code,
    reason = "each test crate uses only part of the shared harness"
)]
mod common;

use std::fmt::Write;
use std::path::Path;
use std::time::{Duration, Instant};

use insta::assert_snapshot;
use rstest::rstest;
use rudo::app::lists::TreeRow;
use rudo::config::Config;
use rudo::storage::watch::{FsChange, PathState};
use rudo::storage::workspace::Entry;
use rudo::ui::lists_pane;

use crate::common::TestApp;

/// Lists whose contents order them differently by every key; today is 2026-03-14.
///
/// | Entry           | open | total | earliest open due | highest open priority |
/// | --------------- | ---- | ----- | ----------------- | --------------------- |
/// | `Home/`         | 2    | 3     | 2026-03-12        | normal                |
/// | `Home/Chores`   | 1    | 2     |                   | normal                |
/// | `Home/Garden`   | 1    | 1     | 2026-03-12        | normal                |
/// | `Work/`, `Plan` | 4    | 4     | 2026-04-01        | highest               |
/// | `Archive`       | 0    | 2     |                   |                       |
/// | `Broken`        | cannot be read                                               |
/// | `Inbox`         | 1    | 1     |                   | normal                |
/// | `Shopping`      | 2    | 3     | 2026-03-20        | normal                |
/// | `Taxes`         | 1    | 4     | 2026-03-10        | high                  |
const WORKSPACE: &[(&str, &[u8])] = &[
    (
        "Archive.md",
        b"- [x] Old\n- [-] Gone [priority:: highest]\n",
    ),
    ("Broken.md", &[0xff]),
    ("Inbox.md", b"- [ ] Call mum\n"),
    (
        "Shopping.md",
        b"- [ ] Milk [due:: 2026-03-20]\n- [ ] Bread\n- [x] Eggs [due:: 2026-03-01]\n",
    ),
    (
        "Taxes.md",
        b"- [ ] File [due:: 2026-03-10] [priority:: high]\n- [x] a\n- [x] b\n- [x] c\n",
    ),
    (
        "Work/Plan.md",
        b"- [ ] A [priority:: highest]\n- [ ] B\n- [ ] C\n- [ ] D [due:: 2026-04-01]\n",
    ),
    ("Home/Garden.md", b"- [ ] Weed [due:: 2026-03-12]\n"),
    (
        "Home/Chores.md",
        b"- [ ] Dust [priority:: low]\n- [x] Mop\n",
    ),
];

const BY_NAME: &[&str] = &[
    "Home/", "Chores", "Garden", "Work/", "Plan", "Archive", "Broken", "Inbox", "Shopping", "Taxes",
];
const BY_DUE: &[&str] = &[
    "Home/", "Garden", "Chores", "Work/", "Plan", "Taxes", "Shopping", "Archive", "Inbox", "Broken",
];
const BY_PRIORITY: &[&str] = &[
    "Work/", "Plan", "Home/", "Garden", "Chores", "Taxes", "Shopping", "Inbox", "Archive", "Broken",
];
const BY_OPEN_TASKS: &[&str] = &[
    "Work/", "Plan", "Home/", "Chores", "Garden", "Shopping", "Inbox", "Taxes", "Archive", "Broken",
];
const BY_TOTAL_TASKS: &[&str] = &[
    "Work/", "Plan", "Home/", "Chores", "Garden", "Taxes", "Shopping", "Archive", "Inbox", "Broken",
];

/// Today in [`TestApp`] is 2026-03-14.
fn workspace() -> TestApp {
    TestApp::from_bytes(WORKSPACE)
}

/// Returns the names of the rows of the lists pane in display order, folders with a trailing
/// `/` and the bin as `Bin`, including rows hidden by a folded folder.
fn row_names(test_app: &TestApp) -> Vec<String> {
    let app = test_app.app();
    app.lists()
        .items(app.workspace())
        .iter()
        .map(|item| match item.row() {
            TreeRow::Entry(Entry::Folder(folder)) => format!("{}/", folder.name()),
            TreeRow::Entry(Entry::List(list)) => list.name().to_owned(),
            TreeRow::Bin(_) => "Bin".to_owned(),
        })
        .collect()
}

/// Returns the names of the rows that the lists pane shows, without those of folded folders.
fn visible_names(test_app: &TestApp) -> Vec<String> {
    let app = test_app.app();
    let items = app.lists().items(app.workspace());
    app.lists()
        .outline()
        .visible_rows(&items)
        .iter()
        .filter_map(|row| row.item().entry())
        .map(|entry| entry.name().to_owned())
        .collect()
}

/// Returns the name of the selected folder or list.
fn selected_name(test_app: &TestApp) -> String {
    let selected = test_app.app().lists().outline().selected().unwrap();
    let entry = test_app.app().workspace().entry(selected.path()).unwrap();
    entry.name().to_owned()
}

/// Writes `content` to the file at `path` as another program would and reports it.
fn write_externally<'app>(
    test_app: &'app mut TestApp,
    path: &str,
    content: &str,
) -> &'app mut TestApp {
    test_app.write(path, content);
    let changes = [FsChange::new(path, PathState::File)];
    test_app.run(|app| app.on_fs_changed(&changes))
}

fn config(yaml: &str) -> Config {
    Config::from_yaml(yaml, Path::new("config.yaml")).unwrap()
}

#[rstest]
#[case::name("s a", BY_NAME)]
#[case::due("s d", BY_DUE)]
#[case::priority("s p", BY_PRIORITY)]
#[case::open_tasks("s o", BY_OPEN_TASKS)]
#[case::total_tasks("s t", BY_TOTAL_TASKS)]
#[case::back_to_name("s t s a", BY_NAME)]
fn sort_keys_order_folders_and_lists(#[case] keys: &str, #[case] expected: &[&str]) {
    let mut test_app = workspace();

    let system_under_test = test_app.press(keys);

    assert_eq!(row_names(system_under_test), expected);
}

#[rstest]
#[case::name("name", BY_NAME)]
#[case::due("due", BY_DUE)]
#[case::priority("priority", BY_PRIORITY)]
#[case::open_tasks("open-tasks", BY_OPEN_TASKS)]
#[case::total_tasks("total-tasks", BY_TOTAL_TASKS)]
fn lists_sort_setting_sets_the_order_at_start(#[case] value: &str, #[case] expected: &[&str]) {
    let config = config(&format!("lists:\n  sort: {value}\n"));

    let system_under_test = TestApp::from_bytes_with_config(WORKSPACE, config);

    assert_eq!(row_names(&system_under_test), expected);
}

#[test]
fn start_opens_the_first_list_of_the_configured_order() {
    let config = config("lists:\n  sort: total-tasks\n");
    let expected = "Plan";

    let system_under_test = TestApp::with_config(&[("A.md", ""), ("Plan.md", "- [ ] a\n")], config);

    assert_eq!(selected_name(&system_under_test), expected);
}

#[rstest]
#[case::name("s a", "Lists")]
#[case::due("s d", "Lists by due")]
#[case::priority("s p", "Lists by priority")]
#[case::open_tasks("s o", "Lists by open tasks")]
#[case::total_tasks("s t", "Lists by total tasks")]
fn title_names_the_active_order(#[case] keys: &str, #[case] expected: &str) {
    let mut test_app = workspace();

    let system_under_test = test_app.press(keys);

    assert_eq!(lists_pane::title(system_under_test.app()), expected);
}

#[test]
fn title_names_the_order_before_hidden_entries_and_the_filter() {
    let mut test_app = workspace();
    let expected = "Lists by due  hidden shown  /a";

    let system_under_test = test_app.press("s d H / a enter");

    assert_eq!(lists_pane::title(system_under_test.app()), expected);
}

#[test]
fn sorting_writes_nothing_to_disk() {
    let mut test_app = workspace();
    let paths = ["Taxes.md", "Shopping.md", "Work/Plan.md"];
    let expected: Vec<String> = paths.iter().map(|path| test_app.read(path)).collect();
    test_app.press("s d s p s o s t");

    let system_under_test: Vec<String> = paths.iter().map(|path| test_app.read(path)).collect();

    assert_eq!(system_under_test, expected);
}

#[test]
fn sort_keys_in_the_tasks_pane_still_sort_tasks() {
    let mut test_app = TestApp::new(&[("Inbox.md", "- [ ] b\n- [ ] a\n")]);
    let expected = "- [ ] a\n- [ ] b\n";
    test_app.press("2 s a");

    let system_under_test = test_app.read("Inbox.md");

    assert_eq!(system_under_test, expected);
}

#[rstest]
#[case::list("G k k", "Inbox")]
#[case::folder("g g j j j", "Work")]
fn sorting_keeps_the_selected_entry(#[case] keys: &str, #[case] expected: &str) {
    let mut test_app = workspace();
    test_app.press(keys);

    let system_under_test = test_app.press("s p");

    assert_eq!(selected_name(system_under_test), expected);
}

#[test]
fn folded_folders_stay_folded_in_another_order() {
    let mut test_app = workspace();
    test_app.press("g g z a");
    let expected = [
        "Home", "Work", "Plan", "Taxes", "Shopping", "Archive", "Inbox", "Broken",
    ];

    let system_under_test = test_app.press("s d");

    assert_eq!(visible_names(system_under_test), expected);
}

#[test]
fn reload_sorts_again_and_keeps_the_selection_and_the_order() {
    let mut test_app = workspace();
    test_app.press("s o G k");
    let expected = [
        "Work/", "Plan", "Home/", "Chores", "Garden", "Inbox", "Shopping", "Taxes", "Archive",
        "Broken",
    ];

    let system_under_test = write_externally(
        &mut test_app,
        "Inbox.md",
        "- [ ] Call mum\n- [ ] Call dad\n- [ ] Call Ann\n",
    );

    assert_eq!(row_names(system_under_test), expected);
    assert_eq!(selected_name(system_under_test), "Archive");
    assert_eq!(
        lists_pane::title(system_under_test.app()),
        "Lists by open tasks"
    );
}

#[test]
fn rescan_keeps_the_order() {
    let mut test_app = workspace();
    test_app.press("s t");
    let expected_first_lists = ["Big", "Taxes"];
    test_app.write("Big.md", "- [ ] a\n- [ ] b\n- [ ] c\n- [ ] d\n- [ ] e\n");
    let changes = [FsChange::new("Big.md", PathState::File)];

    let system_under_test = test_app.run(|app| app.on_fs_changed(&changes));

    assert_eq!(row_names(system_under_test)[5..7], expected_first_lists);
}

#[test]
fn completing_a_task_sorts_again_and_keeps_the_selection_visible() {
    let mut files: Vec<(String, String)> = (1..30)
        .map(|index| (format!("list-{index:02}.md"), "- [ ] a\n".to_owned()))
        .collect();
    files.push(("list-00.md".to_owned(), "- [ ] a\n- [ ] b\n".to_owned()));
    let files: Vec<(&str, &str)> = files
        .iter()
        .map(|(path, content)| (path.as_str(), content.as_str()))
        .collect();
    let mut test_app = TestApp::with_config(&files, config("lists:\n  sort: open-tasks\n"));
    let expected_last = "list-00";
    // The last of 30 rows at the bottom of the 21 rows of the pane in a 24 rows high terminal.
    let expected_offset = 30 - 21;

    let system_under_test = test_app.press("2 x j x");

    assert_eq!(row_names(system_under_test).last().unwrap(), expected_last);
    assert_eq!(selected_name(system_under_test), expected_last);
    assert_eq!(
        system_under_test.app().lists().outline().offset(),
        expected_offset
    );
}

#[test]
fn reload_keeps_the_selection_visible() {
    let mut files: Vec<(String, String)> = (0..30)
        .map(|index| (format!("list-{index:02}.md"), "- [ ] a\n".to_owned()))
        .collect();
    files[0].1.push_str("- [ ] b\n");
    let files: Vec<(&str, &str)> = files
        .iter()
        .map(|(path, content)| (path.as_str(), content.as_str()))
        .collect();
    let mut test_app = TestApp::with_config(&files, config("lists:\n  sort: open-tasks\n"));
    let expected_last = "list-00";
    let expected_offset = 30 - 21;

    let system_under_test = write_externally(&mut test_app, "list-00.md", "- [x] a\n");

    assert_eq!(row_names(system_under_test).last().unwrap(), expected_last);
    assert_eq!(selected_name(system_under_test), expected_last);
    assert_eq!(
        system_under_test.app().lists().outline().offset(),
        expected_offset
    );
}

#[test]
fn undo_sorts_again_and_keeps_the_selection_visible() {
    let mut files: Vec<(String, String)> = (1..30)
        .map(|index| (format!("list-{index:02}.md"), "- [ ] a\n".to_owned()))
        .collect();
    files.push(("list-00.md".to_owned(), "- [x] a\n".to_owned()));
    let files: Vec<(&str, &str)> = files
        .iter()
        .map(|(path, content)| (path.as_str(), content.as_str()))
        .collect();
    let mut test_app = TestApp::with_config(&files, config("lists:\n  sort: open-tasks\n"));
    // Completing the task moves the list to the top, undoing it back to the bottom.
    test_app.press("G enter x");
    let expected_last = "list-00";
    let expected_offset = 30 - 21;

    let system_under_test = test_app.press("u");

    assert_eq!(row_names(system_under_test).last().unwrap(), expected_last);
    assert_eq!(selected_name(system_under_test), expected_last);
    assert_eq!(
        system_under_test.app().lists().outline().offset(),
        expected_offset
    );
}

#[test]
fn visual_mode_selects_rows_of_the_sorted_order() {
    let mut test_app = workspace();
    test_app.press("s t G k k k v j d y");
    let workspace_root = test_app.app().workspace().root().to_path_buf();

    let system_under_test = |path: &str| workspace_root.join(path).exists();

    assert!(!system_under_test("Shopping.md"));
    assert!(!system_under_test("Archive.md"));
    assert!(system_under_test("Taxes.md"));
}

#[test]
fn hidden_entries_sort_like_the_others_and_the_bin_stays_last() {
    let mut test_app = TestApp::new(&[
        ("Inbox.md", "- [ ] a\n"),
        (".Drafts.md", "- [ ] a\n- [ ] b\n"),
        (".trash/Old.md", "- [ ] a\n- [ ] b\n- [ ] c\n"),
    ]);
    let expected = [".Drafts", "Inbox", "Bin"];

    let system_under_test = test_app.press("H s o");

    assert_eq!(row_names(system_under_test), expected);
}

#[rstest]
#[case::hidden("s t", &["Two/", "x", "Five/", "y"])]
#[case::shown("H s t", &["Five/", ".big", "y", "Two/", "x"])]
fn folders_count_only_the_entries_the_pane_shows(#[case] keys: &str, #[case] expected: &[&str]) {
    let mut test_app = TestApp::new(&[
        ("Five/.big.md", "- [ ] a\n- [ ] b\n- [ ] c\n- [ ] d\n"),
        ("Five/y.md", "- [ ] a\n"),
        ("Two/x.md", "- [ ] a\n- [ ] b\n"),
    ]);

    let system_under_test = test_app.press(keys);

    assert_eq!(row_names(system_under_test), expected);
}

#[test]
fn pane_shows_the_order_in_its_title() {
    let mut test_app = workspace();
    test_app.press("s d");

    let system_under_test = test_app.render(80, 14);

    assert_snapshot!(system_under_test);
}

/// 100 lists of 100 tasks each in 10 folders: 10,000 tasks with due dates and priorities.
fn ten_thousand_tasks() -> Vec<(String, String)> {
    let priorities = [
        "",
        " [priority:: high]",
        " [priority:: low]",
        " [priority:: highest]",
    ];
    (0..100)
        .map(|list| {
            let content = (0..100).fold(String::new(), |mut content, task| {
                let status = if (list + task) % 3 == 0 { 'x' } else { ' ' };
                let day = (list * 7 + task) % 28 + 1;
                let priority = priorities[(list + task) % priorities.len()];
                writeln!(
                    content,
                    "- [{status}] Task {task} [due:: 2026-04-{day:02}]{priority}"
                )
                .unwrap();
                content
            });
            (format!("Folder {}/List {list:03}.md", list % 10), content)
        })
        .collect()
}

/// Generous upper bound for one operation on 10,000 tasks, also for unoptimised builds on
/// slow machines; a release build takes a few milliseconds (see the CHANGELOG entry).
const BUDGET: Duration = Duration::from_millis(500);

/// Returns how long `action` takes.
fn timed(action: impl FnOnce()) -> Duration {
    let started = Instant::now();
    action();
    started.elapsed()
}

#[test]
fn sorting_and_reloading_ten_thousand_tasks_stay_fast() {
    let files = ten_thousand_tasks();
    let files: Vec<(&str, &str)> = files
        .iter()
        .map(|(path, content)| (path.as_str(), content.as_str()))
        .collect();
    let mut test_app = TestApp::new(&files);
    let changed = format!("{}- [ ] Added [due:: 2026-03-01]\n", files[42].1);

    let system_under_test = [
        timed(|| {
            test_app.press("s d s p s o s t");
        }) / 4,
        timed(|| {
            test_app.press("j");
        }),
        timed(|| {
            write_externally(&mut test_app, files[42].0, &changed);
        }),
        timed(|| {
            test_app.render(80, 24);
        }),
    ];

    assert!(
        system_under_test.iter().all(|duration| *duration < BUDGET),
        "sort, move, reload, render: {system_under_test:?}"
    );
}

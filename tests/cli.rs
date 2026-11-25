//! Runs the `rudo` binary with `config` subcommands against temporary configuration files.

use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use rstest::rstest;
use rudo::config::DEFAULT_CONFIG_YAML;
use tempfile::TempDir;

/// Isolated home and XDG directories, so that no test reads or writes the user's files.
struct Sandbox {
    directory: TempDir,
}

impl Sandbox {
    fn new() -> Self {
        Self {
            directory: TempDir::new().unwrap(),
        }
    }

    fn path(&self, relative: &str) -> PathBuf {
        self.directory.path().join(relative)
    }

    fn config_dir(&self) -> PathBuf {
        self.path("config/rudo")
    }

    fn write_config(&self, name: &str, yaml: &str) -> PathBuf {
        let path = self.path(name);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, yaml).unwrap();
        path
    }

    fn command(&self, arguments: &[&str]) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_rudo"));
        command
            .args(arguments)
            .env("HOME", self.path("home"))
            .env("XDG_CONFIG_HOME", self.path("config"))
            .env("XDG_DATA_HOME", self.path("data"))
            .env("XDG_STATE_HOME", self.path("state"))
            .env_remove("NO_COLOR")
            .env_remove("RUDO_LOG");
        command
    }

    fn run(&self, arguments: &[&str]) -> Output {
        self.command(arguments).output().unwrap()
    }
}

fn text(bytes: &[u8]) -> &str {
    std::str::from_utf8(bytes).unwrap()
}

fn display(path: &Path) -> String {
    path.display().to_string()
}

#[test]
fn config_default_prints_the_default_configuration_verbatim() {
    let sandbox = Sandbox::new();
    let expected_stdout = DEFAULT_CONFIG_YAML;

    let system_under_test = sandbox.run(&["config", "default"]);

    assert!(system_under_test.status.success());
    assert_eq!(text(&system_under_test.stdout), expected_stdout);
    assert_eq!(text(&system_under_test.stderr), "");
}

#[test]
fn config_default_treats_a_closed_pipe_as_success() {
    let sandbox = Sandbox::new();
    let (reader, writer) = io::pipe().unwrap();
    drop(reader);

    let system_under_test = sandbox
        .command(&["config", "default"])
        .stdout(Stdio::from(writer))
        .output()
        .unwrap();

    assert!(
        system_under_test.status.success(),
        "{}",
        text(&system_under_test.stderr)
    );
}

#[test]
fn config_path_prints_the_explicit_file() {
    let sandbox = Sandbox::new();
    let config_file = sandbox.path("elsewhere.yaml");
    let expected_stdout = format!("{}\n", display(&config_file));

    let system_under_test = sandbox.run(&["config", "path", "--config", &display(&config_file)]);

    assert!(system_under_test.status.success());
    assert_eq!(text(&system_under_test.stdout), expected_stdout);
}

#[test]
fn config_path_prints_the_existing_file_in_the_config_directory() {
    let sandbox = Sandbox::new();
    let config_file = sandbox.write_config("config/rudo/config.yml", "");
    let expected_stdout = format!("{}\n", display(&config_file));

    let system_under_test = sandbox.run(&["config", "path"]);

    assert!(system_under_test.status.success());
    assert_eq!(text(&system_under_test.stdout), expected_stdout);
}

#[test]
fn config_path_prints_the_default_location_when_no_file_exists() {
    let sandbox = Sandbox::new();
    let expected_stdout = format!("{}\n", display(&sandbox.config_dir().join("config.yaml")));

    let system_under_test = sandbox.run(&["config", "path"]);

    assert!(system_under_test.status.success());
    assert_eq!(text(&system_under_test.stdout), expected_stdout);
}

#[test]
fn config_check_accepts_a_valid_file() {
    let sandbox = Sandbox::new();
    let config_file = sandbox.write_config(
        "config/rudo/config.yaml",
        "tasks:\n  indent: 2\nkeybindings:\n  quit: [ctrl-q]\n",
    );
    let expected_stdout = format!("configuration is valid: {}\n", display(&config_file));

    let system_under_test = sandbox.run(&["config", "check"]);

    assert!(system_under_test.status.success());
    assert_eq!(text(&system_under_test.stdout), expected_stdout);
    assert_eq!(text(&system_under_test.stderr), "");
}

#[test]
fn config_check_accepts_the_defaults_without_a_file() {
    let sandbox = Sandbox::new();
    let expected_stdout = "configuration is valid: no configuration file, using the defaults\n";

    let system_under_test = sandbox.run(&["config", "check"]);

    assert!(system_under_test.status.success());
    assert_eq!(text(&system_under_test.stdout), expected_stdout);
}

#[test]
fn config_check_reports_a_yaml_syntax_error_with_its_line() {
    let sandbox = Sandbox::new();
    let config_file = sandbox.write_config(
        "broken.yaml",
        "tasks:\n  indent: tab\n show_completed: true\n",
    );
    let expected_fragments = [display(&config_file), "line 3".to_owned()];

    let system_under_test = sandbox.run(&["--config", &display(&config_file), "config", "check"]);

    assert_eq!(system_under_test.status.code(), Some(1));
    assert_eq!(text(&system_under_test.stdout), "");
    for fragment in expected_fragments {
        assert!(
            text(&system_under_test.stderr).contains(&fragment),
            "{}",
            text(&system_under_test.stderr)
        );
    }
}

#[test]
fn config_check_reports_both_actions_of_a_keymap_conflict() {
    let sandbox = Sandbox::new();
    sandbox.write_config("config/rudo/config.yaml", "keybindings:\n  quit: [j]\n");
    let expected_fragments = ["invalid keybindings", "`quit`", "`move-down`"];

    let system_under_test = sandbox.run(&["config", "check"]);

    assert_eq!(system_under_test.status.code(), Some(1));
    for fragment in expected_fragments {
        assert!(
            text(&system_under_test.stderr).contains(fragment),
            "{}",
            text(&system_under_test.stderr)
        );
    }
}

#[test]
fn config_check_reports_a_missing_explicit_file() {
    let sandbox = Sandbox::new();
    let config_file = sandbox.path("missing.yaml");
    let expected_fragment = format!("{} does not exist", display(&config_file));

    let system_under_test = sandbox.run(&["config", "check", "-c", &display(&config_file)]);

    assert_eq!(system_under_test.status.code(), Some(1));
    assert!(
        text(&system_under_test.stderr).contains(&expected_fragment),
        "{}",
        text(&system_under_test.stderr)
    );
}

#[test]
fn config_subcommands_do_not_create_the_workspace() {
    let sandbox = Sandbox::new();
    let workspace = sandbox.path("data/rudo");

    let system_under_test = ["path", "default", "check"]
        .map(|subcommand| sandbox.run(&["config", subcommand]).status.success());

    assert_eq!(system_under_test, [true; 3]);
    assert!(!workspace.exists());
}

/// What the index of the bin remembers about `Home/Groceries.md`, which is numbered in the bin.
const INDEX: &str = r#"[{"original": "Home/Groceries.md", "location": "Home/Groceries 2.md", "deleted_at": "2026-03-10T12:30:00Z"}]"#;

impl Sandbox {
    /// Creates a workspace whose bin holds a list RuDo deleted and a note another program put
    /// there, and returns its path.
    fn workspace_with_bin(&self) -> PathBuf {
        let workspace = self.path("workspace");
        for (path, content) in [
            ("Inbox.md", "- [ ] Call mum\n"),
            (".trash/Home/Groceries 2.md", "- [ ] Milk\n- [ ] Eggs\n"),
            (".trash/Note.md", "- [ ] Read\n"),
            (".trash/.rudo-bin.json", INDEX),
        ] {
            let file = workspace.join(path);
            fs::create_dir_all(file.parent().unwrap()).unwrap();
            fs::write(file, content).unwrap();
        }
        workspace
    }

    /// Runs `rudo bin <arguments>` on `workspace` in UTC with `input` on stdin.
    fn run_bin(&self, workspace: &Path, arguments: &[&str], input: &str) -> Output {
        let root = display(workspace);
        let arguments: Vec<&str> = ["--root", &root, "bin"]
            .into_iter()
            .chain(arguments.iter().copied())
            .collect();
        let mut child = self
            .command(&arguments)
            .env("TZ", "UTC")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(input.as_bytes())
            .unwrap();
        child.wait_with_output().unwrap()
    }
}

#[test]
fn bin_list_prints_the_entries_newest_first() {
    let sandbox = Sandbox::new();
    let workspace = sandbox.workspace_with_bin();
    let expected_stdout = "2026-03-10 12:30  Home/Groceries.md (in the bin as Home/Groceries 2.md)\n\
                           unknown           Note.md (original path unknown)\n";

    let system_under_test = sandbox.run_bin(&workspace, &["list"], "");

    assert!(system_under_test.status.success());
    assert_eq!(text(&system_under_test.stdout), expected_stdout);
    assert_eq!(text(&system_under_test.stderr), "");
}

#[test]
fn bin_list_of_an_empty_bin_prints_nothing() {
    let sandbox = Sandbox::new();
    let workspace = sandbox.path("workspace");
    fs::create_dir_all(&workspace).unwrap();

    let system_under_test = sandbox.run_bin(&workspace, &["list"], "");

    assert!(system_under_test.status.success());
    assert_eq!(text(&system_under_test.stdout), "");
}

#[test]
fn bin_list_reports_a_missing_workspace() {
    let sandbox = Sandbox::new();
    let workspace = sandbox.path("missing");

    let system_under_test = sandbox.run_bin(&workspace, &["list"], "");

    assert!(!system_under_test.status.success());
    assert!(
        text(&system_under_test.stderr).contains("cannot open the workspace folder"),
        "{}",
        text(&system_under_test.stderr)
    );
}

#[test]
fn bin_empty_with_yes_deletes_everything_without_asking() {
    let sandbox = Sandbox::new();
    let workspace = sandbox.workspace_with_bin();

    let system_under_test = sandbox.run_bin(&workspace, &["empty", "--yes"], "");

    assert!(system_under_test.status.success());
    assert_eq!(text(&system_under_test.stdout), "emptied the bin\n");
    assert_eq!(fs::read_dir(workspace.join(".trash")).unwrap().count(), 0);
    assert!(workspace.join("Inbox.md").is_file());
}

#[rstest]
#[case::y("y\n")]
#[case::yes("Yes\n")]
fn bin_empty_asks_and_deletes_on_yes(#[case] answer: &str) {
    let sandbox = Sandbox::new();
    let workspace = sandbox.workspace_with_bin();
    let expected_stdout = "Delete 2 lists with 3 tasks permanently? This cannot be undone. \
                           [y/N] emptied the bin\n";

    let system_under_test = sandbox.run_bin(&workspace, &["empty"], answer);

    assert!(system_under_test.status.success());
    assert_eq!(text(&system_under_test.stdout), expected_stdout);
    assert!(!workspace.join(".trash/Note.md").exists());
}

#[rstest]
#[case::no("n\n")]
#[case::nothing("")]
#[case::other("delete\n")]
fn bin_empty_keeps_everything_without_a_yes(#[case] answer: &str) {
    let sandbox = Sandbox::new();
    let workspace = sandbox.workspace_with_bin();

    let system_under_test = sandbox.run_bin(&workspace, &["empty"], answer);

    assert_eq!(system_under_test.status.code(), Some(1));
    assert_eq!(text(&system_under_test.stderr), "nothing deleted\n");
    assert!(workspace.join(".trash/Note.md").is_file());
}

#[test]
fn bin_empty_of_an_empty_bin_says_so() {
    let sandbox = Sandbox::new();
    let workspace = sandbox.path("workspace");
    fs::create_dir_all(&workspace).unwrap();

    let system_under_test = sandbox.run_bin(&workspace, &["empty"], "");

    assert!(system_under_test.status.success());
    assert_eq!(text(&system_under_test.stdout), "the bin is empty\n");
}

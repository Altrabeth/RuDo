//! `TestApp`: drives the application like a user does, against a real workspace folder.

use std::fs;
use std::path::Path;

use jiff::civil::{Date, date};
use rudo::app::{App, Effect, execute};
use rudo::config::Config;
use rudo::config::keymap::key::{Key, KeyChord, Modifiers};
use rudo::config::keymap::keymap::Keymap;
use rudo::storage::workspace::{Entry, ListPath, Workspace};
use tempfile::TempDir;

/// The date every `TestApp` treats as today.
pub const TODAY: Date = date(2026, 3, 14);

/// The application with its workspace in a temporary folder and effects executed at once.
pub struct TestApp {
    workspace_folder: TempDir,
    app: App,
}

impl TestApp {
    /// Creates a workspace from `(path, content)` pairs, relative to the workspace root, and
    /// starts the app with the default configuration on [`TODAY`].
    pub fn new(files: &[(&str, &str)]) -> Self {
        let workspace_folder = TempDir::new().unwrap();
        for (path, content) in files {
            let path = workspace_folder.path().join(path);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, content).unwrap();
        }
        let config = Config::default();
        let keymap = Keymap::new(config.keybindings()).unwrap();
        let workspace = Workspace::scan(workspace_folder.path(), config.tasks().indent()).unwrap();
        let app = App::new(workspace, config, keymap, TODAY);
        Self {
            workspace_folder,
            app,
        }
    }

    /// Returns the application.
    pub fn app(&self) -> &App {
        &self.app
    }

    /// Presses the keys written in key notation, such as `"g g"` or `"ctrl-c"`.
    pub fn press(&mut self, keys: &str) -> &mut Self {
        for notation in keys.split_whitespace() {
            let chord: KeyChord = notation.parse().unwrap();
            self.run(|app| app.handle_key(chord));
        }
        self
    }

    /// Types `text` one character after the other.
    pub fn type_text(&mut self, text: &str) -> &mut Self {
        for character in text.chars() {
            let chord = KeyChord::new(Key::Char(character), Modifiers::NONE);
            self.run(|app| app.handle_key(chord));
        }
        self
    }

    /// Runs `step` on the app and executes the effects it returns.
    pub fn run(&mut self, step: impl FnOnce(&mut App) -> Vec<Effect>) -> &mut Self {
        for effect in step(&mut self.app) {
            execute(&mut self.app, effect);
        }
        self
    }

    /// Returns the path of the list at `path`, relative to the workspace root.
    pub fn list_path(&self, path: &str) -> ListPath {
        find_list_path(self.app.workspace().entries(), Path::new(path))
            .unwrap_or_else(|| panic!("the workspace has no list {path}"))
            .clone()
    }

    /// Writes `content` to the file at `path` as another program would.
    pub fn write(&self, path: &str, content: &str) {
        fs::write(self.workspace_folder.path().join(path), content).unwrap();
    }

    /// Reads the file at `path`, relative to the workspace root.
    pub fn read(&self, path: &str) -> String {
        fs::read_to_string(self.workspace_folder.path().join(path)).unwrap()
    }
}

fn find_list_path<'workspace>(
    entries: &'workspace [Entry],
    path: &Path,
) -> Option<&'workspace ListPath> {
    entries.iter().find_map(|entry| match entry {
        Entry::Folder(folder) => find_list_path(folder.entries(), path),
        Entry::List(list) => Some(list.path()).filter(|list_path| list_path.as_path() == path),
    })
}

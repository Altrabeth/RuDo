//! Opening a list in an external editor: the `open-in-editor` action, which editor to run and
//! the command line that opens the list at the selected task.
//!
//! Everything here is pure. The runtime reads `$VISUAL`, `$EDITOR` and the current platform,
//! hands the terminal to the [`EditorCommand`] and reloads the list with [`App::reload_list`]
//! when the editor exits.

use std::ffi::OsString;
use std::io;
use std::num::NonZeroUsize;
use std::path::Path;
use std::process::ExitStatus;

use crate::app::effect::{Effect, error_report};
use crate::app::lists::EntryKey;
use crate::app::state::{App, Focus};
use crate::app::status::StatusMessage;
use crate::platform::Platform;
use crate::storage::workspace::List;

/// Returns the editor command line to run: `configured` from the configuration, else the
/// values of `$VISUAL` and `$EDITOR` in that order, else `vi`, or `notepad` if `platform` is
/// Windows.
///
/// Blank values count as unset, so that an exported but empty `$VISUAL` does not hide
/// `$EDITOR`.
///
/// # Examples
///
/// ```
/// use rudo::app::editor::choose_editor;
/// use rudo::platform::Platform;
///
/// assert_eq!(choose_editor(Platform::Linux, None, Some(""), Some("nano")), "nano");
/// assert_eq!(choose_editor(Platform::Windows, None, None, None), "notepad");
/// ```
#[must_use]
pub fn choose_editor<'command>(
    platform: Platform,
    configured: Option<&'command str>,
    visual: Option<&'command str>,
    editor: Option<&'command str>,
) -> &'command str {
    [configured, visual, editor]
        .into_iter()
        .flatten()
        .find(|command| !command.trim().is_empty())
        .unwrap_or_else(|| fallback_editor(platform))
}

/// Returns the editor used on `platform` when neither the configuration nor the environment
/// names one.
const fn fallback_editor(platform: Platform) -> &'static str {
    match platform {
        Platform::Linux | Platform::MacOs | Platform::OtherUnix => "vi",
        Platform::Windows => "notepad",
    }
}

/// Editors that take the line as `+N file`.
const PLUS_ARGUMENT_EDITORS: [&str; 6] = ["vi", "vim", "nvim", "nano", "emacs", "kak"];
/// Editors that take the line as `file:N`.
const FILE_SUFFIX_EDITORS: [&str; 2] = ["hx", "micro"];

/// How an editor is told the line to open a file at.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LineSyntax {
    /// `+N file`, as vi and its relatives, nano, emacs and kak understand it.
    PlusArgument,
    /// `file:N`, as helix and micro understand it.
    FileSuffix,
    /// The editor is not known to take a line, so it gets just the file.
    Unknown,
}

impl LineSyntax {
    /// Returns the line syntax of the editor `program`, recognised by its name, so that
    /// `/usr/bin/nvim` and `C:/Neovim/bin/nvim.exe` are treated like `nvim`.
    ///
    /// Names are compared ignoring ASCII case, as Windows does for file names.
    fn of(program: &str) -> Self {
        let name = program_name(program);
        let is_one_of = |editors: &[&str]| {
            editors
                .iter()
                .any(|editor| editor.eq_ignore_ascii_case(name))
        };
        if is_one_of(&PLUS_ARGUMENT_EDITORS) {
            Self::PlusArgument
        } else if is_one_of(&FILE_SUFFIX_EDITORS) {
            Self::FileSuffix
        } else {
            Self::Unknown
        }
    }
}

/// The extension of programs on Windows, left out when an editor is recognised by its name.
const EXE_EXTENSION: &str = ".exe";

/// Returns the name of `program` without its folder and without a Windows `.exe` extension
/// (in any case), so that `/usr/bin/nvim` and `C:\Neovim\bin\nvim.exe` are both `nvim`.
///
/// Both separators count on every system: the command comes from the configuration or an
/// environment variable, which may have been written for another system.
fn program_name(program: &str) -> &str {
    let name = program.rsplit(['/', '\\']).next().unwrap_or(program);
    let split = name.len().saturating_sub(EXE_EXTENSION.len());
    match (name.get(..split), name.get(split..)) {
        (Some(stem), Some(extension))
            if !stem.is_empty() && extension.eq_ignore_ascii_case(EXE_EXTENSION) =>
        {
            stem
        }
        _ => name,
    }
}

/// The program and arguments that open a list file in an editor, at a line if the editor
/// supports it.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct EditorCommand {
    program: String,
    arguments: Vec<OsString>,
}

impl EditorCommand {
    /// Builds the command that opens `file` at the 1-based `line` with the editor
    /// `command_line`, such as `nvim` or `code --wait`, on `platform`.
    ///
    /// `command_line` is split into words by the rules of `platform` ([`split_command_line`]),
    /// but nothing is expanded. The arguments of `command_line` come first, then the file:
    /// `+N file` for vi, vim, nvim, nano, emacs and kak, `file:N` for hx and micro and just the
    /// file for every other editor or without a line.
    ///
    /// # Errors
    ///
    /// Returns [`EditorError::Parse`] if `command_line` cannot be split, and
    /// [`EditorError::Empty`] if it has no words.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::num::NonZeroUsize;
    /// use std::path::Path;
    ///
    /// use rudo::app::editor::{EditorCommand, EditorError};
    /// use rudo::platform::Platform;
    ///
    /// let file = Path::new("todo.md");
    /// let command = EditorCommand::new(Platform::Linux, "code --wait", file, NonZeroUsize::new(3))?;
    /// assert_eq!(command.program(), "code");
    /// assert_eq!(command.arguments(), ["--wait", "todo.md"]);
    /// # Ok::<(), EditorError>(())
    /// ```
    pub fn new(
        platform: Platform,
        command_line: &str,
        file: &Path,
        line: Option<NonZeroUsize>,
    ) -> Result<Self, EditorError> {
        let mut words = split_command_line(platform, command_line)?.into_iter();
        let program = words.next().ok_or(EditorError::Empty)?;
        let mut arguments: Vec<OsString> = Vec::with_capacity(words.len() + 2);
        arguments.extend(words.map(OsString::from));
        match line.map(|line| (line, LineSyntax::of(&program))) {
            Some((line, LineSyntax::PlusArgument)) => {
                arguments.extend([OsString::from(format!("+{line}")), file.into()]);
            }
            Some((line, LineSyntax::FileSuffix)) => {
                let mut argument = OsString::from(file);
                argument.push(format!(":{line}"));
                arguments.push(argument);
            }
            Some((_, LineSyntax::Unknown)) | None => arguments.push(file.into()),
        }
        Ok(Self { program, arguments })
    }

    /// Returns the program to run.
    #[must_use]
    pub fn program(&self) -> &str {
        &self.program
    }

    /// Returns the arguments of the program, ending with the file.
    #[must_use]
    pub fn arguments(&self) -> &[OsString] {
        &self.arguments
    }
}

/// Splits the editor `command_line` into the program and its arguments by the rules of
/// `platform`.
///
/// On Linux, macOS and other Unix systems the words are split as a POSIX shell does, so single
/// and double quotes and backslashes work. On Windows they are split as the Microsoft C runtime
/// splits a command line: double quotes group words, and a backslash is literal unless it comes
/// before a double quote, so that `C:\Tools\nvim.exe` stays intact. Single quotes have no
/// meaning there, and an unmatched double quote runs to the end. Because Windows paths often
/// hold spaces, an unquoted program whose first word holds a folder is taken up to the first
/// word that ends in `.exe`, so that `C:\Program Files\Neovim\bin\nvim.exe` needs no quotes.
///
/// # Errors
///
/// Returns [`EditorError::Parse`] if `command_line` has an unmatched quote or ends with a
/// backslash on a Unix system. On Windows every command line can be split.
///
/// # Examples
///
/// ```
/// use rudo::app::editor::{EditorError, split_command_line};
/// use rudo::platform::Platform;
///
/// let command_line = r"C:\Program Files\Neovim\bin\nvim.exe -u NONE";
/// assert_eq!(
///     split_command_line(Platform::Windows, command_line)?,
///     [r"C:\Program Files\Neovim\bin\nvim.exe", "-u", "NONE"],
/// );
/// assert_eq!(split_command_line(Platform::Linux, r"my\ editor")?, ["my editor"]);
/// # Ok::<(), EditorError>(())
/// ```
pub fn split_command_line(
    platform: Platform,
    command_line: &str,
) -> Result<Vec<String>, EditorError> {
    match platform {
        Platform::Linux | Platform::MacOs | Platform::OtherUnix => shell_words::split(command_line)
            .map_err(|source| EditorError::Parse {
                command: command_line.to_owned(),
                source,
            }),
        Platform::Windows => Ok(split_windows_command_line(command_line)),
    }
}

/// Splits `command_line` by the rules of Windows, see [`split_command_line`].
fn split_windows_command_line(command_line: &str) -> Vec<String> {
    let command_line = command_line.trim_start_matches(WINDOWS_WHITESPACE);
    let (program, rest) = unquoted_program_path(command_line)
        .map_or((None, command_line), |(program, rest)| {
            (Some(program), rest)
        });
    program
        .map(str::to_owned)
        .into_iter()
        .chain(windows_words(rest))
        .collect()
}

/// The characters that separate words on a Windows command line.
const WINDOWS_WHITESPACE: [char; 2] = [' ', '\t'];

/// Returns the unquoted program path with spaces at the start of `command_line`, up to and
/// including the first word ending in `.exe`, and the rest of the line, or `None` if the first
/// word holds no folder, is quoted or already ends in `.exe`.
fn unquoted_program_path(command_line: &str) -> Option<(&str, &str)> {
    let first_word = command_line
        .split(WINDOWS_WHITESPACE)
        .next()
        .unwrap_or(command_line);
    let has_folder = first_word.contains(['\\', '/']);
    if !has_folder || first_word.contains('"') || ends_with_exe(first_word) {
        return None;
    }
    let mut end = 0;
    for word in command_line.split_inclusive(WINDOWS_WHITESPACE) {
        end += word.len();
        let word = word.trim_end_matches(WINDOWS_WHITESPACE);
        if word.contains('"') {
            return None;
        }
        if ends_with_exe(word) {
            let (program, rest) = command_line.split_at(end);
            return Some((program.trim_end_matches(WINDOWS_WHITESPACE), rest));
        }
    }
    None
}

/// Whether `word` ends in `.exe`, in any case.
fn ends_with_exe(word: &str) -> bool {
    word.len() > EXE_EXTENSION.len()
        && word
            .get(word.len() - EXE_EXTENSION.len()..)
            .is_some_and(|extension| extension.eq_ignore_ascii_case(EXE_EXTENSION))
}

/// Splits `text` into words as the Microsoft C runtime does: whitespace outside double quotes
/// separates words, `2n` backslashes before a double quote become `n` backslashes and the quote
/// opens or closes a quoted part, `2n + 1` backslashes before it become `n` backslashes and a
/// literal quote, `""` inside a quoted part is a literal quote, and other backslashes are
/// literal.
fn windows_words(text: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut word: Option<String> = None;
    let mut quoted = false;
    let mut backslashes = 0_usize;
    let mut characters = text.chars().peekable();
    while let Some(character) = characters.next() {
        match character {
            '\\' => {
                backslashes += 1;
                continue;
            }
            '"' => {
                let current = word.get_or_insert_with(String::new);
                current.extend(std::iter::repeat_n('\\', backslashes / 2));
                let escaped = backslashes % 2 == 1;
                if escaped || (quoted && characters.next_if_eq(&'"').is_some()) {
                    current.push('"');
                } else {
                    quoted = !quoted;
                }
            }
            ' ' | '\t' if !quoted => {
                push_backslashes(&mut word, backslashes);
                words.extend(word.take());
            }
            _ => {
                push_backslashes(&mut word, backslashes);
                word.get_or_insert_with(String::new).push(character);
            }
        }
        backslashes = 0;
    }
    push_backslashes(&mut word, backslashes);
    words.extend(word);
    words
}

/// Appends `count` literal backslashes to `word`, starting it if needed.
fn push_backslashes(word: &mut Option<String>, count: usize) {
    if count > 0 {
        word.get_or_insert_with(String::new)
            .extend(std::iter::repeat_n('\\', count));
    }
}

/// Why a list could not be opened in the editor.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum EditorError {
    /// The editor command line cannot be split into words.
    #[error("cannot parse the editor command `{command}`")]
    Parse {
        /// The editor command line.
        command: String,
        /// What is wrong with it.
        #[source]
        source: shell_words::ParseError,
    },
    /// The editor command line has no words.
    #[error("the editor command is empty")]
    Empty,
    /// The editor program does not exist.
    #[error("editor {program} not found")]
    NotFound {
        /// The editor program.
        program: String,
    },
    /// The editor program exists but cannot be started.
    #[error("cannot start editor {program}")]
    Start {
        /// The editor program.
        program: String,
        /// The error of the operating system.
        #[source]
        source: io::Error,
    },
    /// The editor exited with a failure.
    #[error("editor {program} failed with {status}")]
    Failed {
        /// The editor program.
        program: String,
        /// How it exited.
        status: ExitStatus,
    },
}

impl App {
    /// Asks to open a list in the editor: in the tasks pane the active list at the line of
    /// the selected task, in the lists pane the selected list at its start.
    ///
    /// A list that could not be loaded is opened as well, so that it can be repaired.
    pub(super) fn open_in_editor(&self) -> Vec<Effect> {
        let request = match self.focus {
            Focus::Lists => match self.lists.outline().selected() {
                Some(EntryKey::List(list)) => Some((list, None)),
                Some(EntryKey::Folder(_) | EntryKey::Bin(_)) | None => None,
            },
            Focus::Tasks => self
                .lists
                .active()
                .map(|list| (list, self.selected_task_line())),
        };
        request
            .map(|(list, line)| Effect::OpenInEditor {
                list: list.clone(),
                line,
            })
            .into_iter()
            .collect()
    }

    /// Returns the 1-based line of the task selected in the tasks pane.
    fn selected_task_line(&self) -> Option<NonZeroUsize> {
        let task = *self.task_outline()?.selected()?;
        let loaded = self.active_list().map(List::content)?.ok()?;
        NonZeroUsize::new(loaded.document().line_number(task)?)
    }

    /// Reports that the editor could not be run or failed with `error`.
    pub fn editor_failed(&mut self, error: &EditorError) {
        let report = error_report(error);
        tracing::warn!(%report, "cannot edit list in the editor");
        self.show_message(StatusMessage::error(report));
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use rstest::rstest;
    use strum::IntoEnumIterator;
    use tempfile::TempDir;

    use super::*;
    use crate::app::state::tests::{app_in, only_list_path};
    use crate::config::keymap::action::Action;

    #[rstest]
    #[case::configured(Some("hx"), Some("nvim"), Some("nano"), "hx")]
    #[case::visual(None, Some("nvim"), Some("nano"), "nvim")]
    #[case::editor(None, None, Some("nano"), "nano")]
    #[case::blank_values_are_unset(Some(" "), Some(""), Some("nano"), "nano")]
    fn editor_is_chosen_in_order_on_every_platform(
        #[case] configured: Option<&str>,
        #[case] visual: Option<&str>,
        #[case] editor: Option<&str>,
        #[case] expected: &str,
    ) {
        let system_under_test: Vec<&str> = Platform::iter()
            .map(|platform| choose_editor(platform, configured, visual, editor))
            .collect();

        assert!(system_under_test.iter().all(|chosen| *chosen == expected));
    }

    #[rstest]
    #[case::linux(Platform::Linux, "vi")]
    #[case::macos(Platform::MacOs, "vi")]
    #[case::other_unix(Platform::OtherUnix, "vi")]
    #[case::windows(Platform::Windows, "notepad")]
    fn without_a_choice_the_fallback_of_the_platform_is_used(
        #[case] platform: Platform,
        #[case] expected: &str,
    ) {
        let blank = Some(" ");

        let system_under_test = choose_editor(platform, blank, blank, None);

        assert_eq!(system_under_test, expected);
    }

    #[rstest]
    #[case::vi("vi", Some(7), "vi", &["+7", "/todo/plan.md"])]
    #[case::vim("vim", Some(7), "vim", &["+7", "/todo/plan.md"])]
    #[case::nvim_by_path("/usr/bin/nvim", Some(7), "/usr/bin/nvim", &["+7", "/todo/plan.md"])]
    #[case::nano("nano", Some(7), "nano", &["+7", "/todo/plan.md"])]
    #[case::emacs_with_option("emacs -nw", Some(7), "emacs", &["-nw", "+7", "/todo/plan.md"])]
    #[case::kak("kak", Some(7), "kak", &["+7", "/todo/plan.md"])]
    #[case::helix("hx", Some(7), "hx", &["/todo/plan.md:7"])]
    #[case::micro("micro", Some(7), "micro", &["/todo/plan.md:7"])]
    #[case::unknown_editor("code --wait", Some(7), "code", &["--wait", "/todo/plan.md"])]
    #[case::without_line("vim", None, "vim", &["/todo/plan.md"])]
    #[case::helix_without_line("hx", None, "hx", &["/todo/plan.md"])]
    #[case::quoted_program(
        "'/opt/my editor/vim' -u NONE",
        Some(2),
        "/opt/my editor/vim",
        &["-u", "NONE", "+2", "/todo/plan.md"]
    )]
    #[case::program_name_only_by_basename("vimlike", Some(2), "vimlike", &["/todo/plan.md"])]
    #[case::notepad("notepad", Some(7), "notepad", &["/todo/plan.md"])]
    #[case::windows_exe("C:/Neovim/bin/nvim.exe", Some(7), "C:/Neovim/bin/nvim.exe", &["+7", "/todo/plan.md"])]
    #[case::exe_in_upper_case("HX.EXE", Some(7), "HX.EXE", &["/todo/plan.md:7"])]
    fn command_opens_the_file_at_the_line(
        #[case] command_line: &str,
        #[case] line: Option<usize>,
        #[case] expected_program: &str,
        #[case] expected_arguments: &[&str],
    ) {
        let file = Path::new("/todo/plan.md");
        let line = line.and_then(NonZeroUsize::new);

        let system_under_test =
            EditorCommand::new(Platform::Linux, command_line, file, line).unwrap();

        assert_eq!(system_under_test.program(), expected_program);
        assert_eq!(system_under_test.arguments(), expected_arguments);
    }

    #[rstest]
    #[case::plain("nvim", "nvim")]
    #[case::unix_path("/usr/bin/nvim", "nvim")]
    #[case::windows_path(r"C:\Neovim\bin\nvim.exe", "nvim")]
    #[case::mixed_separators(r"C:/Tools\hx.exe", "hx")]
    #[case::upper_case_extension("NANO.EXE", "NANO")]
    #[case::only_the_extension(".exe", ".exe")]
    #[case::extension_not_at_the_end("vim.exe.bak", "vim.exe.bak")]
    #[case::multibyte_name("éditeur.exe", "éditeur")]
    fn program_name_drops_the_folder_and_the_exe_extension(
        #[case] program: &str,
        #[case] expected: &str,
    ) {
        let system_under_test = program_name(program);

        assert_eq!(system_under_test, expected);
    }

    #[rstest]
    #[case::empty("")]
    #[case::blank("  ")]
    fn command_without_words_is_an_error(#[case] command_line: &str) {
        let file = Path::new("/todo/plan.md");

        let system_under_test: Vec<Result<EditorCommand, EditorError>> = Platform::iter()
            .map(|platform| EditorCommand::new(platform, command_line, file, None))
            .collect();

        assert!(
            system_under_test
                .iter()
                .all(|command| matches!(command, Err(EditorError::Empty)))
        );
    }

    #[test]
    fn command_with_an_unmatched_quote_is_an_error() {
        let command_line = "'vim";
        let file = Path::new("/todo/plan.md");

        let system_under_test = EditorCommand::new(Platform::Linux, command_line, file, None);

        assert!(matches!(
            system_under_test,
            Err(EditorError::Parse { command, .. }) if command == command_line
        ));
    }

    #[rstest]
    #[case::plain("nvim", &["nvim"])]
    #[case::arguments("code --wait", &["code", "--wait"])]
    #[case::program_path_with_backslashes(
        r"C:\Tools\nvim.exe -u NONE",
        &[r"C:\Tools\nvim.exe", "-u", "NONE"]
    )]
    #[case::unquoted_program_path_with_spaces(
        r"C:\Program Files\Neovim\bin\nvim.exe",
        &[r"C:\Program Files\Neovim\bin\nvim.exe"]
    )]
    #[case::unquoted_program_path_with_spaces_and_arguments(
        r"C:\Program Files\Neovim\bin\NVIM.EXE  -u NONE",
        &[r"C:\Program Files\Neovim\bin\NVIM.EXE", "-u", "NONE"]
    )]
    #[case::quoted_program_path(
        r#""C:\Program Files\Neovim\bin\nvim.exe" -u NONE"#,
        &[r"C:\Program Files\Neovim\bin\nvim.exe", "-u", "NONE"]
    )]
    #[case::forward_slashes("C:/Program Files/Helix/hx.exe", &["C:/Program Files/Helix/hx.exe"])]
    #[case::unquoted_path_without_exe(r"C:\My Tools\vim", &[r"C:\My", r"Tools\vim"])]
    #[case::exe_only_in_an_argument("nvim -u my.exe", &["nvim", "-u", "my.exe"])]
    #[case::leading_whitespace("  \tnotepad  ", &["notepad"])]
    #[case::quoted_part_inside_a_word(
        r#"code --user-data-dir="C:\a b""#,
        &["code", r"--user-data-dir=C:\a b"]
    )]
    #[case::escaped_quote(r#"edit \"quoted\""#, &["edit", r#""quoted""#])]
    #[case::backslashes_before_a_quote(r#"edit "C:\dir\\" next"#, &["edit", r"C:\dir\", "next"])]
    #[case::doubled_quote_inside_quotes(r#"edit "a""b""#, &["edit", r#"a"b"#])]
    #[case::empty_quoted_argument(r#"edit """#, &["edit", ""])]
    #[case::trailing_backslashes(r"edit C:\dir\", &["edit", r"C:\dir\"])]
    #[case::single_quotes_are_literal("edit 'a b'", &["edit", "'a", "b'"])]
    #[case::unmatched_quote_runs_to_the_end(r#"edit "a b"#, &["edit", "a b"])]
    #[case::multi_byte(
        r"C:\Programme\Éditeur Plus\ed.exe",
        &[r"C:\Programme\Éditeur Plus\ed.exe"]
    )]
    #[case::empty("", &[])]
    fn windows_command_lines_are_split_by_windows_rules(
        #[case] command_line: &str,
        #[case] expected: &[&str],
    ) {
        let system_under_test = split_command_line(Platform::Windows, command_line).unwrap();

        assert_eq!(system_under_test, expected);
    }

    #[rstest]
    #[case::backslash_escapes(r"C:\Tools\nvim.exe", &["C:Toolsnvim.exe"])]
    #[case::single_quotes("'/opt/my editor/vim' -u NONE", &["/opt/my editor/vim", "-u", "NONE"])]
    #[case::escaped_space(r"my\ editor", &["my editor"])]
    fn unix_command_lines_are_split_by_posix_shell_rules(
        #[case] command_line: &str,
        #[case] expected: &[&str],
    ) {
        let system_under_test: Vec<Vec<String>> = Platform::iter()
            .filter(|platform| *platform != Platform::Windows)
            .map(|platform| split_command_line(platform, command_line).unwrap())
            .collect();

        assert!(system_under_test.iter().all(|words| words == expected));
    }

    #[rstest]
    #[case::unquoted(r"C:\Program Files\Neovim\bin\nvim.exe")]
    #[case::quoted(r#""C:\Program Files\Neovim\bin\nvim.exe""#)]
    fn windows_editor_path_with_spaces_opens_the_file_at_the_line(#[case] command_line: &str) {
        let file = Path::new(r"C:\Users\alice\Documents\RuDo\plan.md");
        let expected_program = r"C:\Program Files\Neovim\bin\nvim.exe";
        let expected_arguments = ["+7", r"C:\Users\alice\Documents\RuDo\plan.md"];

        let system_under_test =
            EditorCommand::new(Platform::Windows, command_line, file, NonZeroUsize::new(7))
                .unwrap();

        assert_eq!(system_under_test.program(), expected_program);
        assert_eq!(system_under_test.arguments(), expected_arguments);
    }

    /// Starts the fallback editor of Windows for real and stops it again, which shows that
    /// `notepad` is found without a folder.
    #[cfg(windows)]
    #[test]
    fn windows_fallback_editor_starts() {
        let program = choose_editor(Platform::Windows, None, None, None);

        let mut system_under_test = std::process::Command::new(program).spawn().unwrap();

        system_under_test.kill().unwrap();
        system_under_test.wait().unwrap();
    }

    #[test]
    fn lists_pane_opens_the_selected_list_at_its_start() {
        let directory = TempDir::new().unwrap();
        fs::write(directory.path().join("plan.md"), "- [ ] a\n").unwrap();
        let app = app_in(directory.path());
        let list = only_list_path(&app);
        let expected = Effect::OpenInEditor { list, line: None };

        let system_under_test = app.open_in_editor();

        assert_eq!(system_under_test, [expected]);
    }

    #[test]
    fn lists_pane_with_a_folder_selected_opens_nothing() {
        let directory = TempDir::new().unwrap();
        fs::create_dir(directory.path().join("home")).unwrap();
        let app = app_in(directory.path());

        let system_under_test = app.open_in_editor();

        assert_eq!(system_under_test, []);
    }

    #[test]
    fn tasks_pane_without_tasks_opens_the_list_at_its_start() {
        let directory = TempDir::new().unwrap();
        fs::write(directory.path().join("plan.md"), "# Plan\n").unwrap();
        let mut app = app_in(directory.path());
        let list = only_list_path(&app);
        app.focus = Focus::Tasks;
        let expected = Effect::OpenInEditor { list, line: None };

        let system_under_test = app.open_in_editor();

        assert_eq!(system_under_test, [expected]);
    }

    #[test]
    fn tasks_pane_without_an_active_list_opens_nothing() {
        let directory = TempDir::new().unwrap();
        let mut app = app_in(directory.path());
        app.focus = Focus::Tasks;

        let system_under_test = app.update(Action::OpenInEditor);

        assert_eq!(system_under_test, []);
    }

    #[test]
    fn editor_failure_becomes_an_error_message() {
        let directory = TempDir::new().unwrap();
        let mut app = app_in(directory.path());
        let error = EditorError::Start {
            program: "vim".to_owned(),
            source: io::Error::other("permission denied"),
        };
        let expected = StatusMessage::error("cannot start editor vim: permission denied");
        app.editor_failed(&error);

        let system_under_test = app.status_message();

        assert_eq!(system_under_test, Some(&expected));
    }
}

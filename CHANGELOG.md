# Changelog

All notable changes to RuDo are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this
project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- **Windows installer.** Every release has `rudo-tui-x86_64-pc-windows-msvc.msi`, which
  installs RuDo for all users into `C:\Program Files\rudo-tui`, adds `rudo` to `PATH` and
  appears in Installed apps, where it is uninstalled. It is not signed, so SmartScreen asks
  to confirm (**More info**, **Run anyway**). The installation guide now also says that the
  PowerShell install script adds `rudo` to `PATH` and the zip archive does not.
- **Creation dates.** Every task created in RuDo, including the next occurrence of a
  recurring task, gets today's date as `[created:: YYYY-MM-DD]`, the creation date of the
  Obsidian Tasks plugin. Pasted tasks keep their fields and get none. Turn it off with
  `tasks.created_date: false`.
- **Completion in the due date and recurrence prompts.** `tab` (`complete`, now also bound in
  text input) completes the word before the cursor from the words the prompt understands:
  `fr` to `friday` in the due date prompt (`D`), `ev` to `every` and `every 2 w` to
  `every 2 weeks` in the recurrence prompt (`r`). Ambiguous words cycle through the
  candidates, which the hint lists, and back to the typed text; the preview follows. The
  prompts for names ignore `tab`.
- **Sorting the lists pane.** `s d`, `s p`, `s o`, `s t` and `s a` order the folders and lists
  of every folder by urgency (the earliest due date of the open tasks), priority, open tasks,
  total tasks or name, folders first and ordered by everything below them, ties by name.
  The order is a view that changes no file, is named in the pane title (`Lists by due`), stays
  for the session and follows every edit and reload; `lists.sort` sets it at start. Ordering a
  workspace of 10,000 tasks takes well under a millisecond in a release build.
- **Query terms.** The search (`/`) of both panes and go to anything (`ctrl-p`) find tasks
  by their data: `due:today`, `due:<=fri`, `due:overdue`, `due:none`, `created:>-1m`,
  `done:<=-1w`, `priority:>=medium`, `#tag` (also nested tags) and `is:open`, combined with
  text words, all of which must match. With a term, the lists pane shows the lists that
  contain a matching task with the number of matches, and go to anything offers only tasks.
  An invalid value such as `due:blah` shows an error next to the query. `tab` in the search
  completes terms (`due:to` to `due:today`), and the help screen lists them under "Query
  terms". Date expressions everywhere also accept offsets into the past: `-3d`, `-1w`,
  `-1m`, `-1y`.

### Changed

- The sort keys have their own section on the help screen and in the keybinding tables,
  `Sorting (lists pane and tasks pane)`.
- **Search words match on their own.** The words of a search query match in any order, each
  with its own smart case, so `milk buy` finds `Buy milk`.

## [1.0.0] - 2026-11-25

The first release. The [documentation](https://github.com/Altrabeth/RuDo/blob/main/docs/README.md) specifies the behaviour in detail.

### Added

- **License.** RuDo is free software under the GNU General Public License, version 3 or
  later (`GPL-3.0-or-later`); the full text is in `LICENSE`.
- **Storage.** A workspace folder is shown as a tree of folders and lists; every `.md` file
  is a list and every Markdown checkbox a task. Files are loaded losslessly: front matter,
  headings, prose, code blocks, line endings and a byte order mark are kept, and only edited
  task lines are re-rendered. Changes are saved immediately with atomic writes that keep
  symlinks and permissions. Lists that cannot be read stay in the tree and say why.
- **Lists pane.** Create, rename and delete lists and folders (`o`, `O`, `r`, `dd`), with
  names Obsidian accepts and a confirmation before deleting; deleted lists and folders, also
  non-empty ones, go to a bin folder in the workspace (`.trash/`, `workspace.bin`); `done/total` counters; folding;
  a `Bin` row at the end of the tree that shows the bin with original paths and deletion
  dates, restores entries (`R`, undoable), deletes them permanently (`dd`) and empties the bin
  (`E`), with `rudo bin list` and `rudo bin empty` on the command line;
  showing hidden folders and lists such as `.obsidian/` dimmed (`H`, `workspace.show_hidden`).
- **Tasks pane.** Nested tasks with tree guides and folding; adding tasks and subtasks
  (`o`, `O`, `a`) and editing them in place (`i`); marking done (`x`) with an optional
  completion date; deleting (`dd`), moving (`J`, `K`), indenting (`>`, `<`); hiding completed
  tasks (`H` in the tasks pane).
- **Task metadata** in the Dataview format of the Obsidian Tasks plugin: priorities (`+`,
  `-`), due dates with relative input and colour-coded relative display (`D`), Obsidian
  `#tags`, and recurrence rules (`r`) whose completion creates the next occurrence. Unknown
  inline fields are kept.
- **Productivity.** Smart-case search and filtering of both panes and the help screen (`/`),
  undo and redo across lists (`u`, `ctrl-r`) of task edits and of creating, renaming,
  moving and deleting lists and folders, yank and paste of task blocks, also between
  lists (`yy`, `p`, `P`), sorting siblings by priority, due date, text or status (`s p`,
  `s d`, `s a`, `s s`), and opening the list in an external editor at the selected task (`e`).
- **Go to anything.** `ctrl-p` (the action `go-to`) opens a fuzzy finder over every folder,
  list and task of the workspace, like the "go to file" of an editor. Each match shows its
  kind and a task its list (`Buy milk · Home/Groceries`), completed tasks dimmed; names rank
  before paths, folders and lists before tasks and open tasks before completed ones. `enter`
  selects a folder, opens a list, or opens the list of a task and selects it, revealing it if
  the search filter or hidden completed tasks hide it. Inside the picker `ctrl-p` selects the
  previous match; `go-to: ctrl-space` in the configuration opens the finder with `ctrl-space`.
- **Fuzzy folder picker.** Moving lists and folders (`m`) and restoring entries of the bin
  whose original folder is unknown (`R`) ask for the folder with a picker like fzf: the
  matching folders are listed while you type, the best match first with the matched letters
  highlighted; `up`/`down` or `ctrl-p`/`ctrl-n` select, `tab` completes and cycles, `enter`
  accepts.
- **Visual mode.** `v` selects several tasks or lists, vim style; `d` deletes the selection
  (asking first for several tasks and for lists, which go to the bin), `y` yanks the selected tasks and `p`/`P`
  paste every yanked block, so tasks can be moved between lists in one go. `m` moves the
  selected lists and folders, or the selected entry, into another folder. In the bin, `R`
  restores the selected entries in one undoable step and `d` deletes them permanently.
- **Live reload.** The workspace is watched; lists changed by other programs are reloaded,
  new, renamed and removed entries appear, and a save never overwrites a file that changed
  on disk.
- **Interface.** A lazygit-style layout that adapts to the terminal size, a bottom bar with
  the most important keys and messages, empty panes that name the key for the first entry,
  and a scrollable, searchable help screen generated from the effective keymap.
- **Configuration** in `~/.config/rudo/config.yaml`: workspace folder, task settings, editor,
  a theme of named slots with ANSI, hex and 256-colour values (`NO_COLOR` respected), and
  rebindable vim-style keys including key sequences, validated for conflicts with located
  error messages.
- **Command line.** `--root` and `--config`, and `rudo config path`, `rudo config default`
  and `rudo config check`.
- File logging with `RUDO_LOG` filter directives. Without a terminal on standard input and
  output, as in `docker run` without `-it`, `rudo` exits at once with
  `rudo needs an interactive terminal`.
- **Platforms.** Linux, macOS, Windows and FreeBSD. On Windows the configuration is read from
  `%APPDATA%\rudo\config.yaml`, the log goes to `%LOCALAPPDATA%\rudo\rudo.log` and the
  default workspace is `RuDo` in the `Documents` folder that Windows reports, also when
  OneDrive moved it; editor commands are split by Windows rules, `notepad` is the last editor
  fallback, and saving keeps the read-only attribute and works while Obsidian has the list
  open. On macOS live reload also works below symbolic links. Paths inside the workspace are
  shown with `/` on every platform.
- **Installation.** `cargo install --locked rudo-tui` from crates.io (the crate is named
  `rudo-tui` because `rudo` is taken; the command stays `rudo`). Every GitHub release has
  archives for Linux (x86_64 and aarch64, glibc and static musl with the time zone database),
  macOS (x86_64 and aarch64), Windows (x86_64) and FreeBSD (x86_64) with SHA-256 checksums,
  install scripts for the shell and PowerShell, a Homebrew formula
  (`brew install altrabeth/tap/rudo-tui`, also `cargo binstall rudo-tui`), `.deb` and `.rpm`
  packages for x86_64 and aarch64 and an Arch package for x86_64, a minimal Docker image
  `ghcr.io/altrabeth/rudo-tui` for `linux/amd64` and `linux/arm64`, and
  `rudo-tui-<version>-vendor.tar.xz` with the crates for offline builds.
  `nix run github:Altrabeth/RuDo` works through `flake.nix`, and `packaging/` holds the
  recipes for the AUR, Fedora Copr, openSUSE OBS, Gentoo GURU, winget and Chocolatey
  ([`PACKAGING.md`](https://github.com/Altrabeth/RuDo/blob/main/docs/PACKAGING.md)).

[Unreleased]: https://github.com/Altrabeth/RuDo/compare/v1.0.0...HEAD
[1.0.0]: https://github.com/Altrabeth/RuDo/releases/tag/v1.0.0

# Changelog

All notable changes to RuDo are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this
project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- **Windows.** The configuration is read from `%APPDATA%\rudo\config.yaml`, the log goes to
  `%LOCALAPPDATA%\rudo\rudo.log` and the default workspace is `%USERPROFILE%\Documents\RuDo`;
  `XDG_*` variables are honoured there too. `notepad` is the last editor fallback.

### Changed

- Paths inside the workspace are shown and stored in the index of the bin with `/` on every
  platform; the index is read with `/` or `\`.

## [1.0.0] - Unreleased

The first release, not tagged yet. The [README](README.md) specifies the behaviour in detail.

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
- File logging with `RUDO_LOG` filter directives.

[Unreleased]: https://github.com/Altrabeth/RuDo/compare/v1.0.0...HEAD
[1.0.0]: https://github.com/Altrabeth/RuDo/releases/tag/v1.0.0

# RuDo

A keyboard-driven terminal UI for todo lists that live in plain Markdown files.

[![Buy me a coffee](https://img.shields.io/badge/Buy%20me%20a%20coffee-ffdd00?logo=buy-me-a-coffee&logoColor=black)](https://buymeacoffee.com/altrabeth)

RuDo shows a folder of Markdown files as todo lists in a lazygit-style, multi-pane interface
with vim keybindings. Every list is an ordinary `.md` file and every task is an ordinary
Markdown checkbox, so you can point RuDo at a folder inside your Obsidian vault and work on
the same tasks from the terminal and from Obsidian at the same time.

```text
╭ Lists ─────────────────╮╭ Groceries  1/4 ─────────────────────────────────────────────╮
│ ▾ Home                 ││ ▾ [ ] Buy milk #shopping                           tomorrow │
│     Chores         0/2 ││ │   [x] Check lactose-free                                  │
│   • Groceries      1/4 ││   [ ] Bake bread                                   !! in 3d │
│ ▾ Work                 ││   [ ] Water plants                    every week 2d overdue │
│     Inbox          0/1 ││                                                             │
╰────────────────────────╯╰─────────────────────────────────────────────────────────────╯
 o new task  a subtask  i edit  x done  dd delete  D due date                     ? help
```

> **Status:** 1.0, the first stable release. Changes are listed in the
> [changelog](CHANGELOG.md).

## Features

- **Plain Markdown storage.** Lists are `.md` files, folders group lists, tasks are GitHub
  flavoured task list items. Everything RuDo does not understand (front matter, headings,
  prose, code blocks) is preserved byte for byte.
- **Obsidian compatible.** Due dates, priorities and recurrence use the Dataview inline field
  format understood by the Obsidian Tasks plugin, and `#tags` are Obsidian tags.
- **Vim keybindings** for navigation (`j`, `k`, `gg`, `G`, `ctrl-d`, `ctrl-u`), editing
  (`o`, `O`, `i`, `dd`, `yy`, `p`, `>`, `<`), folding (`za`, `zM`, `zR`) and undo (`u`,
  `ctrl-r`).
- **Discoverable.** The bottom bar always shows the most important keys for the focused pane,
  empty panes tell you which key creates the first entry, and `?` opens a scrollable,
  searchable help screen generated from your *effective* keymap, so it always matches your
  configuration.
- **Nested tasks** with folding, reordering (`J`, `K`), indenting (`>`, `<`) and sorting
  (`s p`, `s d`, `s a`, `s s`); the same keys, plus `s o` and `s t`, order the lists pane by
  priority, urgency, open or total tasks without changing a file.
- **Due dates** with relative input (`today`, `tomorrow`, `+3d`, `fri`, `2026-12-24`) and
  colour-coded relative display (`tomorrow`, `in 3d`, `2d overdue`).
- **Priorities, tags and recurring tasks.** Completing a recurring task creates its next
  occurrence.
- **Search and filter** any pane with `/`, with vim-style smart case.
- **Go to anything** with `ctrl-p`: a fuzzy finder over every folder, list and task of the
  workspace that jumps to the chosen one.
- **Fuzzy folder picker** like fzf for moving lists and folders (`m`), with `tab` completion.
- **Undo and redo** for every change to a task, across lists, and for creating, renaming,
  moving and deleting lists and folders.
- **A bin for deleted lists.** Deleted lists and folders go to `.trash/` in the workspace,
  the folder Obsidian uses for deleted notes, so a mistyped `dd` is never lost; browse,
  restore and empty it in the lists pane or with `rudo bin`.
- **Yank and paste** tasks with their subtasks, also between lists.
- **Live reload.** Changes made by other programs (Obsidian, your editor, `git pull`) appear
  immediately; RuDo never silently overwrites them.
- **Open in your editor** at the selected task with `e`.
- **Responsive layout.** Narrow terminals show only the focused pane.
- **Configurable** colours and keybindings in `~/.config/rudo/config.yaml`
  (`%APPDATA%\rudo\config.yaml` on Windows).

## Installation

With Rust 1.100 or newer, installed for example with [rustup](https://rustup.rs):

```sh
cargo install --locked rudo-tui
```

The package is named `rudo-tui`, the command it installs is `rudo`. The crate named `rudo` is
an unrelated tool with a command of the same name; if it is installed, cargo refuses to
overwrite its binary until you remove it with `cargo uninstall rudo`.

Without Rust, the install script downloads the binary for your system, or use Homebrew:

```sh
# Linux and macOS
curl --proto '=https' --tlsv1.2 -LsSf https://github.com/Altrabeth/RuDo/releases/latest/download/rudo-tui-installer.sh | sh
brew install altrabeth/tap/rudo-tui
```

```powershell
# Windows
powershell -ExecutionPolicy Bypass -c "irm https://github.com/Altrabeth/RuDo/releases/latest/download/rudo-tui-installer.ps1 | iex"
```

The install scripts put `rudo` into `~/.cargo/bin` and add it to `PATH`. On Windows, the
installer `rudo-tui-x86_64-pc-windows-msvc.msi` of the
[release](https://github.com/Altrabeth/RuDo/releases/latest) installs into Program Files and
adds `rudo` to `PATH` too; the `.zip` archive does not.

[`docs/installation.md`](docs/installation.md) describes the Windows installer, the `.deb`,
`.rpm` and Arch packages, Nix, Docker, building from source and the other methods.

## Quick start

```sh
rudo --root ~/Obsidian/Vault/Todo
```

1. Press `o` to create your first list and type its name, then `enter`.
2. Press `enter` (or `l`) to open the list. The tasks pane gets the focus.
3. Press `o` to add a task, type it, press `enter`. Press `a` to add a subtask.
4. Press `x` to mark the selected task as done, `D` to give it a due date.
5. Press `?` at any time to see every available key.

Without `--root`, RuDo uses `workspace.root` from the configuration file, and falls back to
`~/.local/share/rudo` (more precisely `$XDG_DATA_HOME/rudo`; `%USERPROFILE%\Documents\RuDo` on
Windows), which is created on first start.
A folder given with `--root` or `workspace.root` must exist.

## Documentation

To find out more about the following topics, read here:

- [Installation](docs/installation.md) on every platform
- [Files, tasks and Obsidian](docs/files-and-obsidian.md): how your files are organised, the
  task syntax and using RuDo with Obsidian
- [Keybindings](docs/keybindings.md): every key, visual mode and the bin
- [Configuration](docs/configuration.md): settings, keybindings, theme and colours
- [Command line](docs/command-line.md): `rudo`, `rudo config` and `rudo bin`
- [Platforms](docs/platforms.md): Linux, macOS, Windows, FreeBSD and Docker
- [Troubleshooting](docs/troubleshooting.md): logs, live reload and terminal quirks
- [Development](docs/development.md): building, testing and contributing

[`docs/README.md`](docs/README.md) lists every document, including those for developers.

## Support

RuDo is free and stays free. If it is useful to you, you can support its development by
[buying me a coffee](https://buymeacoffee.com/altrabeth).

## License

RuDo is free software: you can redistribute it and modify it under the terms of the
[GNU General Public License](LICENSE) as published by the Free Software Foundation, either
version 3 of the License or (at your option) any later version. Anyone may use, study, change
and share RuDo; software that is distributed on the basis of it must be released under the
same license, with its source code.

Copyright (C) 2026 the RuDo authors.

## Acknowledgements

RuDo is inspired by [dooit](https://github.com/dooit-org/dooit) and
[lazygit](https://github.com/jesseduffield/lazygit), and builds on
[ratatui](https://ratatui.rs).

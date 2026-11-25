# Manual QA checklist

The automated tests drive the application core (`tests/scenarios.rs` and the other files in
`tests/`) and the real binary in a pseudo-terminal (`just smoke`). What they cannot see is how
RuDo looks and feels in real terminals, next to real programs. Run this checklist before every
release, tick each item that behaves as described, and note anything that does not. Keep the
filled-in copy of a release in [`docs/qa/`](qa/), named after the date and the version.

## Preparation

```sh
just check && just smoke
just install
rm -rf /tmp/rudo-qa && cp -r demo /tmp/rudo-qa
```

Record the environment:

- RuDo version (`rudo --version`):
- Operating system:
- Terminal emulator and version:
- tmux version:
- Neovim version:
- Obsidian version and Tasks plugin version:
- Date and tester:

Unless an item says otherwise, start RuDo with `rudo --root /tmp/rudo-qa`.

## Terminals

- [ ] **kitty or alacritty**: the interface draws without artefacts; borders, the fold
      markers `▸` `▾` and the scrollbar of the help screen use box-drawing characters, not
      replacement glyphs.
- [ ] The default theme follows the terminal colour scheme (try a light and a dark scheme).
- [ ] `q` and `ctrl-c` restore the terminal: the shell prompt is back, the cursor is visible,
      typed text echoes, and the scrollback from before RuDo is intact.
- [ ] **inside tmux**: the same items as above pass inside a tmux session.
- [ ] Inside tmux, `esc` closes prompts without a noticeable delay (with `escape-time` at its
      default and at `0`).
- [ ] Inside tmux, a hex colour from the theme looks right with the `RGB` terminal feature
      enabled, and the warning in [Theme](configuration.md#theme) describes what happens
      without it.

## Sizes and resizing

- [ ] At **80x24** both panes, the bottom bar with its key hints, and the help screen fit
      without cut-off words in the hints.
- [ ] In a **tiny terminal** (for example 20x5) RuDo says that the terminal is too small and
      does not panic; growing the terminal again brings the interface back.
- [ ] A narrow terminal shows only the focused pane with a hint, and `tab` switches panes.
- [ ] **Resizing** while a prompt, a dialog, the help screen or an inline edit is open
      redraws everything at once and keeps the typed text.
- [ ] Resizing quickly back and forth leaves no stale cells.

## Colours

- [ ] With **`NO_COLOR=1 rudo --root /tmp/rudo-qa`** no colours are used; the selection,
      the focused pane, done, cancelled and overdue tasks are still distinguishable through
      bold, dim, reversed and crossed-out text.
- [ ] With `NO_COLOR` set to an empty value colours are shown (the variable only counts when
      it is non-empty).

## Obsidian side by side

Open `/tmp/rudo-qa` as a vault (or a folder inside a vault) in Obsidian with the Tasks plugin
set to the Dataview task format, and RuDo next to it.

- [ ] Ticking a task in Obsidian appears in RuDo within a second, with a reload message.
- [ ] A task added, completed, given a due date and a priority in RuDo appears in Obsidian,
      and the Tasks plugin reads the due date and priority.
- [ ] A note with front matter, headings, prose and a code block keeps every line except the
      changed task lines (compare with `git diff --no-index` against a copy).
- [ ] Editing the same list in both programs at the same moment never loses the Obsidian
      edit; RuDo says that its last change was not applied.
- [ ] Creating, renaming and deleting a list in Obsidian updates the lists pane.
- [ ] The `.obsidian/` folder is not shown in the lists pane until **`H`** there shows it
      dimmed (title `hidden shown`); `.git/` and `.trash/` never appear in the tree, and **`H`**
      again hides them.
- [ ] A note deleted in Obsidian (**Move to Obsidian trash**) shows up in RuDo's bin after a
      restart with the date `unknown`; **`R`** asks for a folder and puts it there, and
      Obsidian shows it again.

## Editor

- [ ] With `EDITOR=nvim`, **`e`** on a task opens Neovim at the line of that task, RuDo's
      interface is hidden while Neovim runs, and quitting Neovim brings RuDo back.
- [ ] Changes saved in Neovim are shown at once after it exits.
- [ ] `e` with an editor that does not exist reports an error in the bottom bar and keeps
      RuDo usable.
- [ ] `editor: code --wait` (or another command with arguments) in the configuration is used
      instead of `$EDITOR`.

## Configuration

Write each broken file to `/tmp/rudo-qa.yaml` and run
**`rudo config check --config /tmp/rudo-qa.yaml`**. Each must exit with a non-zero status
and report the problem with its line number; starting `rudo --config /tmp/rudo-qa.yaml`
must report the same problem instead of starting.

- [ ] Invalid YAML (for example an unclosed `[`).
- [ ] An unknown setting (`tasks: { show_complete: false }`).
- [ ] An unknown action in `keybindings` (`toggle-dne: x`).
- [ ] A malformed key (`toggle-done: ctrl-`).
- [ ] A key bound to two actions in the same pane (`toggle-done: o`).
- [ ] A binding that is the beginning of another one (`move-to-top: d` clashes with `d d` of
      `delete`). Note that `move-to-top: g` is valid: it replaces `g g`, and no other default
      binding starts with `g`.
- [ ] An unknown theme slot or colour (`theme: { selection: { fg: purple } }`).
- [ ] `--config` pointing to a file that does not exist.
- [ ] A valid file reports success; `rudo config default` piped into a file passes the check.
- [ ] `rudo config path` prints the file in use, also with `XDG_CONFIG_HOME` set.
- [ ] A custom key from the configuration works and appears in the help screen (`?`) and in
      the bottom bar.

## Logs

The log file is `$XDG_STATE_HOME/rudo/rudo.log`; with `XDG_STATE_HOME` unset (the usual case)
that is `~/.local/state/rudo/rudo.log`. `env` sets the variable in every shell, including fish:

```sh
env RUDO_LOG=debug rudo --root /tmp/rudo-qa
tail -f ~/.local/state/rudo/rudo.log   # in a second terminal
```

- [ ] With `RUDO_LOG=debug` the log file receives log lines while you use RuDo, and nothing
      is written over the interface.

## Static Linux binary

`just test-musl` checks that musl builds carry the time zone database, but not that RuDo
falls back to it when the system has none. On Linux with
[bubblewrap](https://github.com/containers/bubblewrap), hide `/usr/share/zoneinfo` from the
static binary of the release (`rudo-tui-x86_64-unknown-linux-musl.tar.xz`, unpacked to
`./rudo`) and list a bin entry deleted at 12:30 UTC:

```sh
mkdir -p /tmp/rudo-tz/.trash && touch /tmp/rudo-tz/.trash/old.md
echo '[{"original": "old.md", "location": "old.md", "deleted_at": "2026-03-10T12:30:00Z"}]' \
  > /tmp/rudo-tz/.trash/.rudo-bin.json
TZ=Pacific/Kiritimati XDG_STATE_HOME=/tmp/rudo-tz-state bwrap --ro-bind / / \
  --bind /tmp /tmp --tmpfs /usr/share/zoneinfo --dev /dev ./rudo --root /tmp/rudo-tz bin list
```

- [ ] The static binary prints `2026-03-11 02:30  old.md`, the local time of UTC+14; a binary
      without the bundled database would fall back to UTC and print `2026-03-10 12:30`.

## Docker image

Once the release workflow has pushed the image, run it on Linux on the workspace of the
previous section and on a fresh folder:

```sh
docker run --rm -e TZ=Pacific/Kiritimati -v /tmp/rudo-tz:/workspace \
  ghcr.io/altrabeth/rudo-tui:X.Y.Z bin list
mkdir -p /tmp/rudo-docker
docker run -it --rm --user "$(id -u):$(id -g)" -v /tmp/rudo-docker:/workspace \
  ghcr.io/altrabeth/rudo-tui:X.Y.Z
```

- [ ] `bin list` prints `2026-03-11 02:30  old.md`.
- [ ] The interface starts; a list created in it appears in `/tmp/rudo-docker`, owned by you,
      and a task added to that list in another terminal appears in RuDo.
- [ ] `docker run --rm ghcr.io/altrabeth/rudo-tui:X.Y.Z` without `-it` exits at once with
      `rudo needs an interactive terminal`.
- [ ] `docker pull ghcr.io/altrabeth/rudo-tui` works without `docker login`, on amd64 and
      arm64 (for example `--platform linux/arm64` with emulation).

## FreeBSD

Run these items on FreeBSD (x86_64) when a release ships a FreeBSD binary; CI runs the tests
in a FreeBSD virtual machine, but not the terminal. Install the toolchain with
[rustup](https://rustup.rs) and `pkg install just`, follow the preparation above, and start
RuDo in the system console and in a terminal emulator under X11 or Wayland.

- [ ] The items of [Terminals](#terminals) pass in the system console and in a terminal
      emulator.
- [ ] A list changed by another program (`echo '- [ ] New' >> /tmp/rudo-qa/Home.md`) shows
      the new task in RuDo without a restart.
- [ ] The configuration, workspace and log folders are those of Linux in
      [Platforms](platforms.md), and `e` opens `vi` with `VISUAL` and `EDITOR` unset.

## Windows

Run these items on Windows 11 when a release ships a Windows binary. CI runs the tests on
Windows, but no smoke test: Python's `pty` module does not exist there, and driving ConPTY
would need a third-party package, so the terminal is checked by hand. Prepare in PowerShell:

```powershell
just install
Remove-Item -Recurse -Force $env:TEMP\rudo-qa -ErrorAction Ignore
Copy-Item -Recurse demo $env:TEMP\rudo-qa
rudo --root $env:TEMP\rudo-qa
```

- [ ] **Windows Terminal with PowerShell**: the items of [Terminals](#terminals) and
      [Sizes and resizing](#sizes-and-resizing) pass; every key acts once (key releases are
      ignored), also `j` held down.
- [ ] The same in the classic console window (`conhost`) started from the Start menu.
- [ ] Without `--root`, `rudo config path` shows `%APPDATA%\rudo\config.yaml`, the log is in
      `%LOCALAPPDATA%\rudo\rudo.log`, and the workspace is `RuDo` in the `Documents` folder,
      also when Explorer shows that folder in OneDrive.
- [ ] With `VISUAL` and `EDITOR` unset, `e` opens the list in `notepad`; closing it brings
      RuDo back with the changes.
- [ ] With `$env:EDITOR = 'C:\Program Files\Neovim\bin\nvim.exe'` (unquoted, and again
      with double quotes around the path), `e` on a task opens Neovim at its line.
- [ ] Paths of entries in the bin (the `Bin` row of the lists pane) use `/`.
- [ ] A list marked read-only in Explorer (Properties) can be changed in RuDo and is still
      read-only afterwards.
- [ ] **A vault synced with Linux** (Syncthing, git or a shared folder): lists with CRLF and LF
      line endings keep theirs after a change, the bin restores entries deleted on the other
      system, and a change on Linux appears in RuDo on Windows after the sync.
- [ ] With the vault open in Obsidian on Windows, the items of
      [Obsidian side by side](#obsidian-side-by-side) pass; no save fails while Obsidian has
      the list open.

## macOS

Run these items on macOS when a release ships a macOS binary. CI runs the tests and the smoke
test on macOS, but not the terminal emulators.

- [ ] **Terminal** and **iTerm2**: the items of [Terminals](#terminals) and
      [Sizes and resizing](#sizes-and-resizing) pass.
- [ ] A list changed by another program (`echo '- [ ] New' >> /tmp/rudo-qa/Home.md`) shows
      the new task in RuDo without a restart; `/tmp` is a symbolic link on macOS.
- [ ] Renaming the list `Home` to `home` (only the case) works and keeps its tasks.
- [ ] The configuration, workspace and log folders are those of Linux in
      [Platforms](platforms.md), and `e` opens `vi` with `VISUAL` and `EDITOR` unset.

## Result

- Items that failed, with the issue opened for each:
- Notes:

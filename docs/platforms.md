# Platforms

RuDo runs on Linux, macOS, Windows and FreeBSD (x86_64); CI runs the test suite on all four.
On Windows, use Windows Terminal or PowerShell in it; on macOS, Terminal or iTerm2. The
behaviour is the same everywhere except for these locations, the last editor fallback and how
the editor command is split into words:

| What               | Linux, macOS and FreeBSD       | Windows                          |
| ------------------ | ------------------------------ | -------------------------------- |
| configuration file | `~/.config/rudo/config.yaml`   | `%APPDATA%\rudo\config.yaml`     |
| default workspace  | `~/.local/share/rudo`          | `RuDo` in the `Documents` folder |
| log file           | `~/.local/state/rudo/rudo.log` | `%LOCALAPPDATA%\rudo\rudo.log`   |
| editor fallback    | `vi`                           | `notepad`                        |
| editor command     | split like a POSIX shell       | split like a Windows program     |

The `Documents` folder is the one Windows reports, so a folder moved to OneDrive or redirected
elsewhere is followed; without one, RuDo uses `%USERPROFILE%\Documents`. On Windows a
backslash in the editor command is part of the path, double quotes group words, and an
unquoted program path with spaces is read up to its `.exe`, so both
`C:\Program Files\Neovim\bin\nvim.exe` and `"C:\Program Files\Neovim\bin\nvim.exe" -u NONE`
work. Elsewhere, quote or escape spaces as in a shell.

On every platform, `XDG_CONFIG_HOME`, `XDG_DATA_HOME` and `XDG_STATE_HOME` replace these
folders (with a `rudo` subfolder) when they hold an absolute path. If `%APPDATA%` or
`%LOCALAPPDATA%` is unset, RuDo uses `%USERPROFILE%\AppData\Roaming` or
`%USERPROFILE%\AppData\Local`. Paths inside the workspace are always shown with `/`, and the
index of the bin stores them with `/` too, so a workspace synced between Windows and Linux
reads the same; RuDo reads both `/` and `\`. New files are written with LF line endings on
every platform, as Obsidian does; existing files keep theirs.

## Docker

The Docker image holds only the static binary ([installation](installation.md#docker)). Mount
your workspace at `/workspace`:

```sh
docker run -it --rm --user "$(id -u):$(id -g)" -v ~/Vault/Todo:/workspace ghcr.io/altrabeth/rudo-tui
```

- `-it` gives RuDo the terminal it needs; without it, RuDo stops at once with
  `rudo needs an interactive terminal`.
- `--user "$(id -u):$(id -g)"` makes the lists RuDo creates belong to you; without it they
  belong to `root` on the host.
- The tag `latest` is the newest release; `ghcr.io/altrabeth/rudo-tui:1.0.0` pins a version.
- Arguments after the image name go to `rudo`, which always runs with `--root /workspace`. To
  use your configuration, mount its folder and name the file:

  ```sh
  docker run -it --rm --user "$(id -u):$(id -g)" -v ~/Vault/Todo:/workspace \
      -v ~/.config/rudo:/config:ro ghcr.io/altrabeth/rudo-tui --config /config/config.yaml
  ```

- The container runs in UTC, which decides what "today" is for due dates. Pass your time zone
  with `-e TZ=Europe/Berlin`, or mount `-v /etc/localtime:/etc/localtime:ro` on Linux.
- `NO_COLOR` is passed on with `-e NO_COLOR`. Docker sets `TERM=xterm` inside the container,
  which is enough for RuDo, since it needs no terminfo database; hex colours depend on your
  terminal, as without Docker.
- The image has no editor, so `e` (`open-in-editor`) does not work in it.
- Live reload works on bind mounts on Linux. Docker Desktop on macOS and Windows does not pass
  every change of the host to the container, so changes by other programs may only appear
  after restarting RuDo.

`docker build -t rudo-tui .` builds the image from a checkout; it needs BuildKit, the default
builder of Docker (on some Linux distributions the `docker-buildx` package).

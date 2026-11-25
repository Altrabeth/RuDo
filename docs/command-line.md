# Command line

```text
rudo [OPTIONS] [COMMAND]

Commands:
  config path      Print the path of the configuration file in use
  config default   Print the default configuration
  config check     Validate the configuration file
  bin list         List the lists and folders in the bin
  bin empty        Delete everything in the bin permanently

Options:
  -r, --root <DIR>       Workspace folder (overrides `workspace.root`)
  -c, --config <FILE>    Configuration file (default: see `rudo config path`)
  -h, --help             Print help
  -V, --version          Print version
```

Without a command, `rudo` starts the interface, which needs a terminal on standard input and
output; if either is redirected, as in `docker run` without `-it`, it exits with status 1.

Without `--root`, RuDo uses `workspace.root` from the configuration file, and falls back to
`~/.local/share/rudo` (more precisely `$XDG_DATA_HOME/rudo`; `%USERPROFILE%\Documents\RuDo` on
Windows), which is created on first start.
A folder given with `--root` or `workspace.root` must exist.

## `rudo config`

```sh
rudo config path      # print the configuration file RuDo uses
rudo config default   # print the complete default configuration with comments
rudo config check     # validate your configuration and report errors with line numbers
rudo config default > ~/.config/rudo/config.yaml   # start from the defaults
```

`rudo config check` exits with status 1 and prints every problem if the configuration is
invalid. [Configuration](configuration.md) describes the file.

## `rudo bin`

`rudo bin list` prints one line per entry of the bin of the workspace (`--root` or
`workspace.root`), the most recently deleted first: the deletion time, the original path and,
if it differs, the path inside the bin; `unknown` for entries another program put there.
`rudo bin empty` asks on stdin before it deletes everything in the bin permanently, unless
`--yes` is given, and exits with status 1 if something could not be deleted.

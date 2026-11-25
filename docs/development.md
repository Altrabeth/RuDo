# Development

The project uses [just](https://github.com/casey/just) as a command runner. Run `just` to list
all recipes.

| Recipe               | Purpose                                                        |
| -------------------- | -------------------------------------------------------------- |
| `just install-tools` | install `cargo-deny`, `cargo-insta` and `cargo-llvm-cov`       |
| `just build`         | build RuDo                                                     |
| `just install`       | install the `rudo` binary into `~/.cargo/bin`                  |
| `just run <args>`    | run RuDo from source, e.g. `just run --root /tmp/todo`         |
| `just demo`          | run RuDo on a fresh copy of the sample workspace in `demo/`    |
| `just fmt`           | format all code                                                |
| `just lint`          | clippy with warnings denied                                    |
| `just check-targets` | clippy for Windows, macOS and FreeBSD from any system (installs the targets) |
| `just test`          | run all tests including doctests                               |
| `just test-musl`     | run all tests for static musl Linux (installs the target)      |
| `just smoke`         | run the binary in a pseudo-terminal through the quick start (needs `python3`) |
| `just snap`          | review changed snapshots of the rendered interface             |
| `just doc`           | build the API documentation with warnings denied               |
| `just cov`           | test coverage (`just cov --open` for the HTML report)          |
| `just deny`          | check dependencies for advisories and licenses                 |
| `just publish-dry-run` | list the files of the crate and build it as crates.io would  |
| `just packaging-aur` | build and lint the Arch package of the current commit (see [`PACKAGING.md`](PACKAGING.md)) |
| `just check`         | everything that must pass before a commit                      |

## Workflow

Development is test driven: write a failing test, make it pass, refactor. The architecture
keeps the program logic in a pure core (state, update, rendering) that is tested without a
terminal, while a thin shell owns the terminal and the file system. Guard tests compare the
keybinding tables of [`keybindings.md`](keybindings.md) and the colour and theme tables of
[`configuration.md`](configuration.md) with the code, so they fail when one changes without
the other. `tests/doc_links.rs` checks every relative link and heading anchor in the Markdown
files of the repository.

## Further documents

- [`ARCHITECTURE.md`](ARCHITECTURE.md): the module layout and the testing strategy.
- [`QA.md`](QA.md): the manual checklist in real terminals before a release.
- [`RELEASING.md`](RELEASING.md): the release procedure.
- [`PACKAGING.md`](PACKAGING.md): the recipes for package managers.
- [`AGENTS.md`](../AGENTS.md): the coding rules every change must follow.

## Continuous integration

CI runs `just check`, `just smoke` and the coverage on Linux for every push and pull request.
The lints and tests on Windows and macOS, and the tests in a FreeBSD virtual machine, run when
the workflow is started by hand and on pushes to `main`. Pushing a `v*` tag builds the GitHub
release with [dist](https://github.com/axodotdev/cargo-dist) (`.github/workflows/release.yml`). `.gitattributes` checks every text file out with LF line endings and the
fixtures in `tests/fixtures/` byte for byte, so the byte-exact tests mean the same on Windows.

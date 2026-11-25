# Installation

RuDo 1.0 is not released yet. Building from source works today; every other method below is
marked with the release that makes it work. Once a method is live, it moves into the
installation section of the [README](../README.md).

| System  | Methods                                                                               |
| ------- | ------------------------------------------------------------------------------------- |
| Linux   | [source](#from-source), [crates.io](#cratesio), [release binaries](#release-binaries-and-install-scripts), [Homebrew](#homebrew-and-cargo-binstall), [.deb, .rpm and Arch packages](#deb-rpm-and-arch-packages), [AUR](#aur), [Nix](#nix), [Docker](#docker) |
| macOS   | [source](#from-source), [crates.io](#cratesio), [release binaries](#release-binaries-and-install-scripts), [Homebrew](#homebrew-and-cargo-binstall), [Nix](#nix) |
| Windows | [source](#from-source), [crates.io](#cratesio), [release binaries](#release-binaries-and-install-scripts), [winget and Chocolatey](#winget-and-chocolatey) |
| FreeBSD | [source](#from-source), [crates.io](#cratesio), [release binaries](#release-binaries-and-install-scripts) |

## From source

To build RuDo from source you need Rust 1.100 or newer, installed for example with
[rustup](https://rustup.rs). In a checkout, the compiler version pinned in
`rust-toolchain.toml` is installed automatically:

```sh
git clone https://github.com/Altrabeth/RuDo
cd RuDo
cargo install --locked --path .   # or: just install
```

`--locked` builds with the dependency versions RuDo is tested with. This installs the `rudo`
binary into `~/.cargo/bin`. The crate named `rudo` on crates.io is an unrelated tool with a
command of the same name; if it is installed, cargo refuses to overwrite its binary until you
remove it with `cargo uninstall rudo`.

*Available with the 1.0 release:* the development version is installed from the repository
without a checkout:

```sh
cargo install --locked --git https://github.com/Altrabeth/RuDo rudo-tui
```

## crates.io

*Available with the 1.0 release.*

RuDo is published on [crates.io](https://crates.io/crates/rudo-tui) as `rudo-tui`; the command
it installs is `rudo`. It needs the same Rust version as a build [from source](#from-source):

```sh
cargo install --locked rudo-tui
```

## Release binaries and install scripts

*Available with the 1.0 release.*

Every [GitHub release](https://github.com/Altrabeth/RuDo/releases) has ready-made binaries
for Linux (x86_64 and aarch64, linked to glibc or fully static with musl), macOS (Intel and
Apple silicon), Windows (x86_64) and FreeBSD (x86_64), each archive with its SHA-256 checksum.
The static Linux binaries carry the time zone database, so they also know the local day on
systems without `/usr/share/zoneinfo`, such as minimal containers. The install scripts pick
the archive for your system and put `rudo` into `~/.cargo/bin`:

```sh
# Linux and macOS
curl --proto '=https' --tlsv1.2 -LsSf https://github.com/Altrabeth/RuDo/releases/latest/download/rudo-tui-installer.sh | sh
```

```powershell
# Windows
powershell -ExecutionPolicy Bypass -c "irm https://github.com/Altrabeth/RuDo/releases/latest/download/rudo-tui-installer.ps1 | iex"
```

The macOS binaries are not notarized, because RuDo has no Apple Developer account. Homebrew
and the install script are not affected; a `rudo` unpacked from a downloaded archive is
blocked by Gatekeeper until you remove the quarantine attribute:

```sh
xattr -d com.apple.quarantine ./rudo
```

## Homebrew and cargo-binstall

*Available with the 1.0 release.*

With [Homebrew](https://brew.sh) on macOS and Linux, or with
[cargo-binstall](https://github.com/cargo-bins/cargo-binstall), which downloads the same
release archives instead of compiling:

```sh
brew install altrabeth/tap/rudo-tui
cargo binstall rudo-tui
```

## .deb, .rpm and Arch packages

*Available with the 1.0 release.*

Every release also has `.deb` and `.rpm` packages for x86_64 and aarch64 (`amd64` and `arm64`
for Debian) and an Arch package for x86_64, so that your package manager installs and removes
RuDo. Download the one for your system from the
[release page](https://github.com/Altrabeth/RuDo/releases/latest) and install it, here version
1.0.0:

```sh
sudo apt install ./rudo-tui_1.0.0-1_amd64.deb                            # Debian, Ubuntu
sudo dnf install ./rudo-tui-1.0.0-1.x86_64.rpm                           # Fedora, RHEL
sudo zypper install --allow-unsigned-rpm ./rudo-tui-1.0.0-1.x86_64.rpm   # openSUSE
sudo pacman -U ./rudo-tui-1.0.0-1-x86_64.pkg.tar.zst                     # Arch Linux
```

The `.deb` and `.rpm` packages hold the static binary, so they work on old releases too.

## AUR

*Available after the 1.0 release, once the package is submitted.*

On Arch Linux, install the AUR package `rudo-tui` (for example `paru -S rudo-tui`).

## Nix

*Available with the 1.0 release.*

With [Nix](https://nixos.org), run `nix run github:Altrabeth/RuDo`.

## Docker

*Available with the 1.0 release.*

Every release is also a minimal Docker image for `linux/amd64` and `linux/arm64`, holding only
the static binary, on the GitHub Container Registry. Mount your workspace at `/workspace`:

```sh
docker run -it --rm --user "$(id -u):$(id -g)" -v ~/Vault/Todo:/workspace ghcr.io/altrabeth/rudo-tui
```

[Platforms](platforms.md#docker) explains the options, time zones, colours and live reload in
the container.

## winget and Chocolatey

*Available after the 1.0 release, once the packages are accepted.*

The manifests for winget (`Altrabeth.RuDo`) and Chocolatey (`rudo-tui`) are kept in
`packaging/`.

## Other package managers

The recipes for the AUR, Nix, Fedora Copr, openSUSE OBS, Gentoo GURU, winget and Chocolatey
are kept in `packaging/` and `flake.nix`; [`PACKAGING.md`](PACKAGING.md) explains them for
packagers.

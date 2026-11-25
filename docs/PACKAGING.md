# Packaging RuDo

The recipes for the package managers that RuDo's maintainer submits to live in this repository,
so that a submission after a release is a copy-and-submit step. Packagers of other systems are
welcome to start from them. The GitHub release itself, with binaries, install scripts, the
Homebrew formula and the `.deb`, `.rpm` and Arch packages that CI builds, is described in
[`RELEASING.md`](RELEASING.md).

| Channel                   | Recipe                                        | Built by CI                         |
| ------------------------- | --------------------------------------------- | ----------------------------------- |
| AUR and `makepkg`         | `packaging/aur/rudo-tui/PKGBUILD`, `.SRCINFO` | `makepkg` and `namcap` on Arch      |
| `.deb` and `.rpm` release | `[package.metadata]` in `Cargo.toml`          | `cargo deb`, `cargo generate-rpm`   |
| Nix                       | `flake.nix`, `flake.lock`                     | `nix flake check`, `nix run`        |
| Fedora Copr, openSUSE OBS | `packaging/rpm/rudo-tui.spec`                 | no, try it in a container (below)   |
| Gentoo GURU               | `packaging/gentoo/app-misc/rudo-tui/`         | no                                  |
| winget                    | `packaging/winget/` (three manifests)         | no, needs Windows                   |
| Chocolatey                | `packaging/chocolatey/`                       | no, needs Windows                   |

## Facts every recipe shares

- The package is named `rudo-tui`, the command it installs is `rudo`, the license is
  `GPL-3.0-or-later`. The names `rudo` and `rudo-bin` belong to unrelated projects on the AUR,
  and `rudo` on crates.io too, so no recipe uses them as a package name.
- Builds from source use the source archive of the release,
  `https://github.com/Altrabeth/RuDo/releases/download/vX.Y.Z/source.tar.gz`, which holds the
  tagged tree in a folder `rudo-tui-X.Y.Z/` (dist packs it with `git archive`).
- Offline builders take the crates from `rudo-tui-X.Y.Z-vendor.tar.xz` of the same release: the
  output of `cargo vendor --versioned-dirs` in a folder `vendor/`. The release job
  `custom-build-vendor-tarball` builds it with `just vendor-tarball`, which packs the files
  sorted, with the commit time and without owners, so the same commit and toolchain always give
  the same bytes; the `.sha256` file next to it has its checksum.
- Building needs Rust 1.100 or newer (`rust-version` in `Cargo.toml`). Builds run
  `cargo build --release` with `Cargo.lock` (`--frozen` or `--locked`) and run the tests, as an
  unprivileged user: root ignores the file permissions that the tests of failed deletes rely on.
- The Windows packages download `rudo-tui-x86_64-pc-windows-msvc.zip`, which holds `rudo.exe`
  without a folder; dist links the C runtime statically, so they need no redistributable.

## Versions of pre-releases

A pre-release such as `1.0.0-rc.1` must sort before its release `1.0.0`, but the package formats
reject the hyphen or read it as the separator of the package revision. So every package carries
the version in the syntax of its format, which one function computes: `package_version` in
`packaging/update.py`. `python3 packaging/update.py package-version FORMAT VERSION` prints it for
the recipes of the justfile, and `just packaging-update` writes it into the recipes.

| Format              | `1.0.0-rc.1` becomes | Where                                                  |
| ------------------- | -------------------- | ------------------------------------------------------ |
| Debian (`deb`)      | `1.0.0~rc.1`         | `cargo deb --deb-version` in `just linux-packages`     |
| RPM (`rpm`)         | `1.0.0~rc.1`         | `cargo generate-rpm --set-metadata`, `Version` of spec |
| Arch Linux (`arch`) | `1.0.0rc1`           | `pkgver` of the PKGBUILD and `.SRCINFO`                |
| Gentoo (`gentoo`)   | `1.0.0_rc1`          | the name of the ebuild                                 |

A `~` sorts before anything in Debian and RPM; an alphabetic part after the release sorts before
it in `vercmp`; Gentoo has the suffixes `_alpha`, `_beta`, `_pre` and `_rc`. Further hyphens of
the pre-release become dots. A release version is the same in every format. The script refuses a
pre-release that Arch cannot sort (one that starts with a digit) or Gentoo cannot express (one
other than `alpha`, `beta`, `pre` or `rc` with an optional number).

Download links, archive names and the folder in the source archive keep the version of the tag:
the PKGBUILD holds it in `_pkgver`, the spec in `%{upstream_version}`, the ebuild in `MY_PV`.
winget and Chocolatey (2.0 and newer) accept SemVer versions, so their manifests carry the
version unchanged. `just package-version-order` maps a list of ascending versions and checks in
Debian, Fedora and Arch containers that `dpkg --compare-versions`, `rpm.vercmp` and `vercmp`
keep their order; the `version-order` job of `.github/workflows/packaging.yml` runs it on every
push.

## After a release

When the release workflow has published `vX.Y.Z`, in a checkout whose `Cargo.toml` and
`Cargo.lock` are those of the tag (the development branch right after the release):

```sh
just packaging-update X.Y.Z
git diff
```

`packaging/update.py` downloads the `.sha256` files of the source archive and the Windows
archive from the release and sets the version, the checksums and the release links in the
PKGBUILD, `.SRCINFO`, the RPM spec, the winget manifests and the Chocolatey package. It adds a
changelog entry to the spec under the name of the last one, renames the ebuild to the new
version (in its Gentoo form) and writes its `CRATES` from `Cargo.lock`, and copies
`rust-version` into the spec and
the ebuild. `flake.nix` reads the version from `Cargo.toml` and needs nothing. The script stops
without changing anything if `Cargo.toml` of the checkout has another version. Commit the
result (`chore(packaging): update the recipes to X.Y.Z`), merge it into `main` like any change,
and submit each channel as described below. `just test-packaging`, part of `just check`, tests
the script on a copy of the recipes.

Before the first release the recipes hold `1.0.0` and checksums of zeros, which
`just packaging-update 1.0.0` replaces.

## Arch Linux: AUR and makepkg

The PKGBUILD follows the
[Rust package guidelines](https://wiki.archlinux.org/title/Rust_package_guidelines): `prepare`
fetches the crates of `Cargo.lock`, `build` runs `cargo build --frozen --release` (with debug
information for the `-debug` package), `check` runs `cargo test --frozen`, and `package`
installs `/usr/bin/rudo`, the README and changelog to `/usr/share/doc/rudo-tui/` and the license
to `/usr/share/licenses/rudo-tui/`. It conflicts with `rudo` and `rudo-bin`, which install a
command `rudo` too.

Without the AUR, anyone builds and installs the latest release from a checkout of `main`, where
the recipes are updated after every release:

```sh
cd packaging/aur/rudo-tui && makepkg -si
```

`just packaging-aur` builds it from the current commit instead of a release: it packs the tree
with `git archive` the way dist packs `source.tar.gz`, runs `makepkg --skipchecksums` on a copy
in `target/packaging/aur/` whose `_pkgver` is the version of `Cargo.toml` and `pkgver` its
Arch form, checks that `.SRCINFO` matches `makepkg --printsrcinfo` and lints the
PKGBUILD and the package with `namcap` (the `-debug` package is left out, because namcap
reports its build-id links into the main package). The `arch` job of
`.github/workflows/packaging.yml` runs it in an `archlinux:base-devel` container on every push
and installs the package with `scripts/package-smoke.sh`. The release attaches the package that
`just package-arch` builds from the same PKGBUILD; there is no second Arch recipe.

**Submitting.** Once: create an account on [aur.archlinux.org](https://aur.archlinux.org) with
an SSH key. For every release:

```sh
git clone ssh://aur@aur.archlinux.org/rudo-tui.git aur-rudo-tui   # the first clone is empty
cp packaging/aur/rudo-tui/PKGBUILD packaging/aur/rudo-tui/.SRCINFO aur-rudo-tui/
cd aur-rudo-tui && makepkg --cleanbuild && git add PKGBUILD .SRCINFO
git commit -m "Update to X.Y.Z" && git push
```

The first push creates the package. Building with the real `source.tar.gz` there verifies its
checksum.

## Nix

`flake.nix` builds RuDo with `rustPlatform.buildRustPackage` from `Cargo.lock`, with the compiler
of `rust-toolchain.toml` from [rust-overlay](https://github.com/oxalica/rust-overlay), so it does
not depend on the Rust version of nixpkgs; the package runs the tests. It offers
`packages.default`, `apps.default` and `checks.default` for Linux and macOS (x86_64 and
aarch64), and `devShells.default` with the pinned toolchain and its components, `just`,
`cargo-deny`, `cargo-insta`, `cargo-llvm-cov` and Python for the smoke test:

```sh
nix run github:Altrabeth/RuDo            # the development branch: github:Altrabeth/RuDo/dev/v1.0
nix profile install github:Altrabeth/RuDo/vX.Y.Z
nix develop                              # in a checkout
```

The `nix` job of `.github/workflows/packaging.yml` runs `nix flake check`, `nix run` and
`nix develop` on every push. Nothing changes per release. To move to newer nixpkgs and
rust-overlay, run `nix flake update` and commit `flake.lock`; a new toolchain in
`rust-toolchain.toml` needs an update of rust-overlay that knows it. Adding RuDo to nixpkgs is
up to a nixpkgs maintainer.

## Fedora Copr and openSUSE OBS

`packaging/rpm/rudo-tui.spec` builds on both from `Source0` (the source archive) and `Source1`
(the vendor archive). `%prep` unpacks both and points cargo to `vendor/`, `%build` runs
`cargo build --frozen --release`, `%check` runs `cargo test --frozen`. It uses plain cargo
instead of the Rust macros of either distribution, so that one file serves both; Fedora's
`%build_rustflags` are used when they are defined. `License` lists RuDo's license and the
licenses of the crates linked into the binary on Linux, which
`cargo tree -e normal --target x86_64-unknown-linux-gnu --format '{l}'` lists; review it when
the dependencies change. Only releases whose `rust` package is 1.100 or newer can build it.

To try the spec before a release, put a `git archive --format=tar.gz --prefix=rudo-tui-1.0.0/
HEAD` as `rudo-tui-1.0.0.tar.gz` and the output of `just vendor-tarball` into
`~/rpmbuild/SOURCES/` and run `rpmbuild -ba packaging/rpm/rudo-tui.spec` as an unprivileged user
on Fedora or openSUSE (a `fedora` or `opensuse/tumbleweed` container works), then `rpmlint` on
the spec and the packages.

**Copr, once.** Log in to [copr.fedorainfracloud.org](https://copr.fedorainfracloud.org) with a
Fedora account, create the project `rudo-tui` with the chroots of the current Fedora releases
(x86_64 and aarch64), and save the API token of **API** to `~/.config/copr` for
[`copr-cli`](https://docs.pagure.org/copr.copr/user_documentation.html). Users install with
`dnf copr enable altrabeth/rudo-tui && dnf install rudo-tui`. **For every release**, build the
source package from the downloaded sources and submit it:

```sh
spectool -g -C ~/rpmbuild/SOURCES packaging/rpm/rudo-tui.spec
rpmbuild -bs packaging/rpm/rudo-tui.spec
copr-cli build rudo-tui ~/rpmbuild/SRPMS/rudo-tui-X.Y.Z-1.*.src.rpm
```

**OBS, once.** Create an account on [build.opensuse.org](https://build.opensuse.org); the home
project `home:<user>` exists then. Add the repositories to build for (**Repositories**, for
example openSUSE Tumbleweed and the current Fedora), and create the package:
`osc checkout home:<user> && cd home:<user> && osc mkpac rudo-tui`. **For every release**, in
`home:<user>/rudo-tui`, replace the spec and the two archives and commit:

```sh
cp .../packaging/rpm/rudo-tui.spec .
osc rm --force *.tar.gz *.tar.xz 2> /dev/null; spectool -g rudo-tui.spec
osc addremove && osc commit -m "Update to X.Y.Z"
```

Users add the repository of the project that OBS shows on the package page under
**Download package**.

## Gentoo GURU

`packaging/gentoo/app-misc/rudo-tui/` holds the ebuild and `metadata.xml` for
[GURU](https://wiki.gentoo.org/wiki/Project:GURU), the user repository of Gentoo. The ebuild uses
`cargo.eclass`: `CRATES` lists every crate of `Cargo.lock` as `name@version`, the format that
`cargo-ebuild` and `pycargoebuild` write, and portage downloads them from crates.io next to the
source archive, so no vendor archive is needed. `RUST_MIN_VER` is the `rust-version` of
`Cargo.toml`. `LICENSE` adds the Gentoo names of the crate licenses (`BSD`, `Boost-1.0`,
`CC0-1.0`, `ISC`, `MIT`, `Unicode-3.0`, `ZLIB`); review it with
`pycargoebuild -i rudo-tui-X.Y.Z.ebuild` when the dependencies change. GURU needs a `Manifest`
with the checksums of every download, which `pkgdev manifest` writes from the published
release; it is not kept here.

**Once:** read the [GURU regulations](https://wiki.gentoo.org/wiki/Project:GURU/Information_for_Contributors),
accept the Gentoo Certificate of Origin and get push access as described there. **For every
release**, in a clone of `guru` on its `dev` branch:

```sh
rm -f app-misc/rudo-tui/rudo-tui-*.ebuild
cp -r .../packaging/gentoo/app-misc/rudo-tui app-misc/
cd app-misc/rudo-tui && pkgdev manifest && pkgcheck scan
ebuild rudo-tui-X.Y.Z.ebuild clean test install
pkgdev commit --signoff -m "app-misc/rudo-tui: add X.Y.Z, drop old" && pkgdev push
```

## winget

`packaging/winget/` holds the multi-file manifest (version, installer and en-US locale,
schema 1.10.0) of the package `Altrabeth.RuDo`: a `zip` installer whose nested `portable`
`rudo.exe` winget installs under the command alias `rudo`. `just packaging-update` writes the
download link and the checksum in upper case, as winget writes them.

**For every release**, on Windows with a fork of
[microsoft/winget-pkgs](https://github.com/microsoft/winget-pkgs):

```powershell
$dir = "manifests\a\Altrabeth\RuDo\X.Y.Z"
New-Item -ItemType Directory $dir; Copy-Item ...\packaging\winget\*.yaml $dir
winget validate --manifest $dir
winget settings --enable LocalManifestFiles; winget install --manifest $dir; rudo --version
```

Commit the folder to a branch of the fork and open a pull request; the moderators and the
validation pipeline of winget-pkgs review it. After the first version,
`wingetcreate update Altrabeth.RuDo --version X.Y.Z --urls <zip URL> --submit` does the same
from the previous manifest.

## Chocolatey

`packaging/chocolatey/rudo-tui.nuspec` describes the package `rudo-tui`, and
`tools/chocolateyinstall.ps1` downloads the Windows archive of the release, checks its SHA-256
and unpacks it into the package folder; Chocolatey creates the shim `rudo` and removes
everything on uninstall. The package embeds no binary, so it needs no `VERIFICATION.txt`.

**Once:** create an account on [community.chocolatey.org](https://community.chocolatey.org) and
copy the API key of the account page. **For every release**, on Windows:

```powershell
cd packaging\chocolatey
choco pack
choco install rudo-tui --source . ; rudo --version ; choco uninstall rudo-tui
choco push rudo-tui.X.Y.Z.nupkg --source https://push.chocolatey.org/ --api-key <key>
```

The package appears after the automated checks and the moderation of the community repository.

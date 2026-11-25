# Short commands for the project tooling. Run `just` to list them.

set positional-arguments

# Deny cargo's own warnings (manifest lints) as CI does, so that a green `just check` means a
# green CI run.
export CARGO_BUILD_WARNINGS := "deny"

directory_name := kebabcase(file_name(justfile_directory()))

# List all recipes
default:
    @just --list --unsorted

# Rename the package and pin the latest stable toolchain, MSRV and edition
init-project name=directory_name: && check
    #!/usr/bin/env bash
    set -euo pipefail
    if [[ ! "$1" =~ ^[a-zA-Z][a-zA-Z0-9_-]*$ ]]; then
        echo "error: invalid package name '$1'" >&2
        exit 1
    fi
    rustup update --no-self-update stable
    version="$(rustc +stable --version | cut -d ' ' -f 2)"
    probe="$(mktemp -d)"
    trap 'rm -rf "$probe"' EXIT
    cargo +stable new --quiet --vcs none "$probe/probe"
    edition="$(sed -n 's/^edition = //p' "$probe/probe/Cargo.toml")"
    sed -i "s/^channel = .*/channel = \"$version\"/" rust-toolchain.toml
    sed -i \
        -e "0,/^name = .*/s//name = \"$1\"/" \
        -e "s/^edition = .*/edition = $edition/" \
        -e "s/^rust-version = .*/rust-version = \"$version\"/" \
        Cargo.toml
    rustup toolchain install
    cargo update

# Install the cargo subcommands used by the recipes
install-tools:
    cargo install --locked cargo-deny cargo-insta cargo-llvm-cov

# Build the project
build *args:
    cargo build "$@"

# Install the rudo binary into ~/.cargo/bin
install:
    cargo install --path . --locked

# Run the binary with the given arguments
run *args:
    cargo run -- "$@"

# Run RuDo on a fresh copy of the sample workspace in `demo/`
demo:
    rm -rf target/demo-workspace
    mkdir -p target
    cp -r demo target/demo-workspace
    cargo run -- --root target/demo-workspace

# Format all code
fmt *args:
    cargo fmt "$@"

# Lint all targets and features, denying warnings
lint:
    cargo clippy --all-targets --all-features -- -D warnings

# Lint for Windows, macOS and FreeBSD from any machine; checking needs no linker or SDK of the target
check-targets: (check-target "x86_64-pc-windows-msvc") (check-target "aarch64-apple-darwin") (check-target "x86_64-unknown-freebsd")

# Lint all targets and features for one compilation target, installing it if needed
check-target target:
    rustup target add {{ target }}
    cargo clippy --all-targets --all-features --target {{ target }} -- -D warnings

# Run all tests, including doctests
test *args:
    cargo test --all-features "$@"

# Run all tests for static musl Linux, whose binaries carry the time zone database
test-musl:
    rustup target add x86_64-unknown-linux-musl
    cargo test --all-features --target x86_64-unknown-linux-musl

# Drive the real binary in a pseudo-terminal through the quick start of the README
smoke: build
    python3 scripts/smoke.py target/debug/rudo

# Build the Docker image with BuildKit and check it without a terminal, as CI does
docker-smoke:
    docker build -t rudo-tui:smoke .
    scripts/docker-smoke.sh rudo-tui:smoke

# Review changed snapshots of the rendered interface
snap:
    cargo insta review

# Build the documentation, denying warnings
doc *args:
    RUSTDOCFLAGS="-D warnings" cargo doc --no-deps "$@"

# Measure test coverage (e.g. `just cov --open` for the HTML report)
cov *args:
    cargo llvm-cov --all-features "$@"

# Check dependencies for advisories, licenses, duplicates and sources
deny:
    cargo deny check

# Package and build the crate as crates.io would; the real `cargo publish` stays manual
publish-dry-run *args:
    cargo package --list "$@"
    cargo publish --dry-run --locked "$@"

# Run every check required before committing
check: (fmt "--check") lint test test-packaging doc deny

# Pack the crates of Cargo.lock reproducibly into rudo-tui-<version>-vendor.tar.xz
vendor-tarball out_dir="target/distrib":
    #!/usr/bin/env bash
    set -euo pipefail
    version="$(sed -n 's/^version = "\(.*\)"$/\1/p' Cargo.toml | head -n 1)"
    archive="rudo-tui-$version-vendor.tar.xz"
    staging="$(mktemp -d)"
    trap 'rm -rf "$staging"' EXIT
    cargo vendor --locked --versioned-dirs --quiet "$staging/vendor" > /dev/null
    mkdir -p "$1"
    tar -C "$staging" --sort=name --mtime="@$(git log -1 --format=%ct)" --owner=0 --group=0 \
        --numeric-owner --mode=go-w,a+rX \
        --pax-option=exthdr.name=%d/PaxHeaders/%f,delete=atime,delete=ctime \
        -cf - vendor | xz -9 --threads=1 > "$1/$archive"
    cd "$1"
    sha256sum "$archive" > "$archive.sha256"
    echo "$1/$archive"

# Build and lint the Arch package of the current commit with makepkg and namcap
packaging-aur: _makepkg
    #!/usr/bin/env bash
    set -euo pipefail
    (cd packaging/aur/rudo-tui && makepkg --printsrcinfo | diff -u .SRCINFO -)
    cd target/packaging/aur
    # The -debug package is left out: namcap flags its build-id links into the main package
    namcap PKGBUILD rudo-tui-[0-9]*.pkg.tar.zst | tee namcap.log
    ! grep -q ' E: ' namcap.log

# Build the Arch package of the current commit into out_dir, with its checksum
package-arch out_dir="target/distrib": _makepkg
    #!/usr/bin/env bash
    set -euo pipefail
    mkdir -p "$1"
    package="$(basename target/packaging/aur/rudo-tui-[0-9]*.pkg.tar.zst)"
    cp "target/packaging/aur/$package" "$1/"
    cd "$1"
    sha256sum "$package" > "$package.sha256"
    echo "$1/$package"

# Run makepkg in target/packaging/aur on the PKGBUILD, set to the version of Cargo.toml, and a
# source archive of the current commit packed like dist's source.tar.gz
_makepkg:
    #!/usr/bin/env bash
    set -euo pipefail
    build="target/packaging/aur"
    rm -rf "$build"
    mkdir -p "$build"
    # The PKGBUILD names the last release until `just packaging-update`, a release the new one
    version="$(sed -n 's/^version = "\(.*\)"$/\1/p' Cargo.toml | head -n 1)"
    pkgver="$(python3 packaging/update.py package-version arch "$version")"
    sed -e "s/^_pkgver=.*/_pkgver=$version/" -e "s/^pkgver=.*/pkgver=$pkgver/" \
        packaging/aur/rudo-tui/PKGBUILD > "$build/PKGBUILD"
    git archive --format=tar.gz --prefix="rudo-tui-$version/" HEAD > "$build/rudo-tui-$version.tar.gz"
    cd "$build"
    makepkg --force --cleanbuild --skipchecksums

# Pack target/<target>/dist/rudo, a static musl binary, into a .deb and an .rpm in out_dir, with
# their checksums
linux-packages target="x86_64-unknown-linux-musl" out_dir="target/distrib":
    #!/usr/bin/env bash
    set -euo pipefail
    # A fresh folder per target, because cargo generate-rpm does not print what it wrote
    staging="target/linux-packages/$1"
    rm -rf "$staging"
    mkdir -p "$staging" "$2"
    # Both formats read the hyphen of a pre-release differently; --deb-version includes the revision
    version="$(sed -n 's/^version = "\(.*\)"$/\1/p' Cargo.toml | head -n 1)"
    deb_version="$(python3 packaging/update.py package-version deb "$version")-1"
    rpm_version="$(python3 packaging/update.py package-version rpm "$version")"
    # The binary stays as dist built it for the archives; stripping would need a cross strip
    cargo deb --no-build --no-strip --target "$1" --profile dist --deb-version "$deb_version" \
        --output "$staging" > /dev/null
    cargo generate-rpm --target "$1" --profile dist --set-metadata "version = \"$rpm_version\"" \
        --output "$staging"
    cd "$staging"
    for package in *; do
        # GitHub renames release assets with a `~` (pre-releases) to a `.`; naming the file that
        # way already keeps the name in its checksum file equal to the downloaded file
        asset="${package//\~/.}"
        [ "$asset" = "$package" ] || mv "$package" "$asset"
        sha256sum "$asset" > "$asset.sha256"
        mv "$asset" "$asset.sha256" "{{ absolute_path(out_dir) }}/"
        echo "$2/$asset"
    done

# Install every package in out_dir in a container of its distribution, run it and remove it
package-smoke out_dir="target/distrib":
    #!/usr/bin/env bash
    set -euo pipefail
    shopt -s nullglob
    for package in "$1"/*.deb; do scripts/package-smoke.sh "$package" debian:stable; done
    for package in "$1"/*.rpm; do
        scripts/package-smoke.sh "$package" fedora:latest
        scripts/package-smoke.sh "$package" opensuse/tumbleweed
    done
    for package in "$1"/*.pkg.tar.zst; do scripts/package-smoke.sh "$package" archlinux:latest; done

# Check in containers that dpkg, rpm and vercmp sort the package versions of pre-releases first
package-version-order:
    scripts/package-version-order.sh deb debian:stable
    scripts/package-version-order.sh rpm fedora:latest
    scripts/package-version-order.sh arch archlinux:latest

# Set the version and checksums of every packaging recipe from a published GitHub release
packaging-update version:
    python3 packaging/update.py "$1"

# Test the packaging update script
test-packaging:
    python3 -B -m unittest discover --start-directory packaging --pattern 'test_*.py'

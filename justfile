# Short commands for the project tooling. Run `just` to list them.

set positional-arguments

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
check: (fmt "--check") lint test doc deny

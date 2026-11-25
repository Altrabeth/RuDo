# RPM recipe for Fedora Copr and openSUSE OBS. Both build without network, so the crates come
# from the vendor archive of the GitHub release (`just vendor-tarball`). The spec uses plain
# cargo instead of the Rust macros of either distribution, so that the same file builds on both.
# `just packaging-update <version>` sets the version after a release; docs/PACKAGING.md
# explains the recipe.

Name:           rudo-tui
Version:        1.0.0
Release:        1%{?dist}
Summary:        Keyboard-driven terminal UI for to-do lists stored as Markdown files
# RuDo is GPL-3.0-or-later; the rest are the licenses of the crates linked into the binary
License:        GPL-3.0-or-later AND BSD-3-Clause AND BSL-1.0 AND CC0-1.0 AND ISC AND MIT AND Unicode-3.0 AND Zlib
URL:            https://github.com/Altrabeth/RuDo
Source0:        %{url}/releases/download/v%{version}/source.tar.gz#/%{name}-%{version}.tar.gz
Source1:        %{url}/releases/download/v%{version}/%{name}-%{version}-vendor.tar.xz

BuildRequires:  cargo >= 1.100
BuildRequires:  gcc
BuildRequires:  rust >= 1.100

%description
RuDo is a keyboard-driven terminal UI for to-do lists stored as plain Markdown
files. The workspace is a folder, every Markdown file is a list and every
checkbox a task, so the same tasks can be edited in RuDo and in Obsidian. The
command is rudo.

%prep
%autosetup -a 1
mkdir -p .cargo
cat > .cargo/config.toml << 'EOF'
[source.crates-io]
replace-with = "vendored-sources"

[source.vendored-sources]
directory = "vendor"
EOF

%build
# Keeps debug information for the debuginfo package; cargo strips it by default
export CARGO_PROFILE_RELEASE_DEBUG=2
%{?build_rustflags:export RUSTFLAGS='%{build_rustflags}'}
cargo build --frozen --release

%install
install -Dpm 0755 target/release/rudo %{buildroot}%{_bindir}/rudo

%check
cargo test --frozen

%files
%license LICENSE
%doc README.md CHANGELOG.md
%{_bindir}/rudo

%changelog
* Wed Nov 25 2026 Altrabeth <Altrabeth@users.noreply.github.com> - 1.0.0-1
- First package of RuDo 1.0.0

#!/bin/sh
# Smoke test of a .deb, .rpm or Arch package of RuDo, as CI and the release run it.
#
# Installs the package with the package manager of the system, runs `rudo --version` and
# `rudo config check`, checks that it holds the documentation, removes the package and checks
# that `rudo` is gone. With IMAGE, the test runs in a fresh container of that image, for the platform of the
# package; without it, on the current system, which needs root (CI runs it so in a container).
#
# Usage: scripts/package-smoke.sh PACKAGE [IMAGE]
set -eu

package="${1:?usage: scripts/package-smoke.sh PACKAGE [IMAGE]}"

if [ -n "${2:-}" ]; then
    case "$package" in
        *amd64.deb | *x86_64.rpm | *x86_64.pkg.tar.zst) platform=linux/amd64 ;;
        *arm64.deb | *aarch64.rpm | *aarch64.pkg.tar.zst) platform=linux/arm64 ;;
        *)
            echo "error: no platform for $package" >&2
            exit 1
            ;;
    esac
    directory="$(cd "$(dirname "$package")" && pwd)"
    script="$(cd "$(dirname "$0")" && pwd)/$(basename "$0")"
    exec docker run --rm --platform "$platform" \
        --volume "$directory:/packages:ro" --volume "$script:/package-smoke.sh:ro" \
        "$2" sh /package-smoke.sh "/packages/$(basename "$package")"
fi

package="$(cd "$(dirname "$package")" && pwd)/$(basename "$package")"
case "$package" in
    *.deb)
        apt-get install --yes "$package"
        files="dpkg --listfiles rudo-tui"
        remove="apt-get remove --yes rudo-tui"
        ;;
    *.rpm)
        files="rpm --query --list rudo-tui"
        if command -v dnf > /dev/null; then
            dnf install --assumeyes "$package"
            remove="dnf remove --assumeyes rudo-tui"
        else
            zypper --non-interactive install --allow-unsigned-rpm "$package"
            remove="zypper --non-interactive remove rudo-tui"
        fi
        ;;
    *.pkg.tar.zst)
        pacman --upgrade --noconfirm "$package"
        files="pacman --query --list --quiet rudo-tui"
        remove="pacman --remove --noconfirm rudo-tui"
        ;;
    *)
        echo "error: unknown package format of $package" >&2
        exit 1
        ;;
esac

rudo --version
rudo config check
# The package database, not the file system: the Fedora image installs no documentation
$files | grep --quiet --line-regexp /usr/share/doc/rudo-tui/README.md
$files | grep --quiet --line-regexp /usr/share/doc/rudo-tui/CHANGELOG.md

$remove
# Not `command -v`, which may answer from the shell's cache of the path
if [ -e /usr/bin/rudo ]; then
    echo "error: /usr/bin/rudo is still installed after removing the package" >&2
    exit 1
fi
echo "package smoke test passed: $(basename "$package")"

#!/usr/bin/env python3
"""Sets the version and checksums of every packaging recipe from a published GitHub release.

Run it in a checkout of the release tag, after the release workflow has published the
release: the checksums come from the `.sha256` files of the release, the crate list of the
ebuild and the minimum Rust version from `Cargo.lock` and `Cargo.toml` of the checkout.
`flake.nix` reads the version from `Cargo.toml` itself and needs no update.

Usage: packaging/update.py VERSION   (or: just packaging-update VERSION)
"""

import dataclasses
import datetime
import re
import sys
import tomllib
import urllib.request
from pathlib import Path

REPOSITORY_ROOT = Path(__file__).resolve().parent.parent
RELEASE_DOWNLOAD_URL = "https://github.com/Altrabeth/RuDo/releases/download/v{version}/{name}"
SOURCE_ARCHIVE = "source.tar.gz"
WINDOWS_ARCHIVE = "rudo-tui-x86_64-pc-windows-msvc.zip"
VERSION_PATTERN = re.compile(r"\d+\.\d+\.\d+(-[0-9A-Za-z.-]+)?")
SHA256_PATTERN = re.compile(r"[0-9a-fA-F]{64}")

PKGBUILD = Path("packaging/aur/rudo-tui/PKGBUILD")
SRCINFO = Path("packaging/aur/rudo-tui/.SRCINFO")
RPM_SPEC = Path("packaging/rpm/rudo-tui.spec")
EBUILD_DIRECTORY = Path("packaging/gentoo/app-misc/rudo-tui")
WINGET_DIRECTORY = Path("packaging/winget")
NUSPEC = Path("packaging/chocolatey/rudo-tui.nuspec")
CHOCOLATEY_INSTALL = Path("packaging/chocolatey/tools/chocolateyinstall.ps1")


class UpdateError(Exception):
    """A recipe or a release does not look as this script expects."""


@dataclasses.dataclass(frozen=True)
class Release:
    """Everything the recipes record about one release."""

    version: str
    source_sha256: str
    windows_sha256: str
    rust_version: str
    crates: list[str]
    date: datetime.date


def set_field(text: str, pattern: str, value: str) -> str:
    """Replaces what follows the prefix matched by group 1 of `pattern` on exactly one line."""
    regex = re.compile(pattern, re.MULTILINE)
    matches = len(regex.findall(text))
    if matches != 1:
        raise UpdateError(f"expected one match of {pattern!r}, found {matches}")
    return regex.sub(lambda match: match.group(1) + value, text)


def parse_sha256_file(text: str) -> str:
    """Returns the checksum of a `.sha256` file in the format of `sha256sum`, in lowercase."""
    checksum = text.split(maxsplit=1)[0] if text.strip() else ""
    if not SHA256_PATTERN.fullmatch(checksum):
        raise UpdateError(f"not a sha256 checksum file: {text[:80]!r}")
    return checksum.lower()


def crate_list(cargo_lock: str) -> list[str]:
    """Returns the crates.io packages of a `Cargo.lock` as `name@version`, in its order."""
    packages = tomllib.loads(cargo_lock)["package"]
    return [
        f"{package['name']}@{package['version']}"
        for package in packages
        if package.get("source", "").startswith("registry+")
    ]


def minimum_rust_version(cargo_toml: str) -> str:
    """Returns the `rust-version` of a `Cargo.toml` with three components."""
    rust_version = tomllib.loads(cargo_toml)["package"]["rust-version"]
    return ".".join((rust_version.split(".") + ["0", "0"])[:3])


def update_pkgbuild(text: str, release: Release) -> str:
    """Sets the version, resets the release number and sets the source checksum."""
    text = set_field(text, r"^(pkgver=).*$", release.version)
    text = set_field(text, r"^(pkgrel=).*$", "1")
    return set_field(text, r"^(sha256sums=\(').*('\))$", release.source_sha256 + "')")


def update_srcinfo(text: str, release: Release) -> str:
    """Updates `.SRCINFO` as `makepkg --printsrcinfo` would print the updated PKGBUILD."""
    text = set_field(text, r"^(\tpkgver = ).*$", release.version)
    text = set_field(text, r"^(\tpkgrel = ).*$", "1")
    text = set_field(
        text,
        r"^(\tsource = ).*$",
        f"rudo-tui-{release.version}.tar.gz::"
        + RELEASE_DOWNLOAD_URL.format(version=release.version, name=SOURCE_ARCHIVE),
    )
    return set_field(text, r"^(\tsha256sums = ).*$", release.source_sha256)


def update_rpm_spec(text: str, release: Release) -> str:
    """Sets the version and the minimum Rust, and adds a changelog entry by the last author."""
    rust = ".".join(release.rust_version.split(".")[:2])
    text = set_field(text, r"^(Version:\s+).*$", release.version)
    text = set_field(text, r"^(Release:\s+).*$", "1%{?dist}")
    text = set_field(text, r"^(BuildRequires:\s+cargo >= ).*$", rust)
    text = set_field(text, r"^(BuildRequires:\s+rust >= ).*$", rust)
    last_entry = re.search(r"^%changelog\n\* \w{3} \w{3} \d{2} \d{4} (.+?) - ", text, re.MULTILINE)
    if last_entry is None:
        raise UpdateError("the spec has no %changelog entry to take the author from")
    entry = (
        f"* {release.date:%a %b %d %Y} {last_entry.group(1)} - {release.version}-1\n"
        f"- Update to {release.version}\n\n"
    )
    return text.replace("%changelog\n", "%changelog\n" + entry, 1)


def update_ebuild(text: str, release: Release) -> str:
    """Replaces the crate list and the minimum Rust version of the ebuild."""
    crates = "".join(f"\t{crate}\n" for crate in release.crates)
    text = set_field(text, r'^(RUST_MIN_VER=").*$', release.rust_version + '"')
    return set_field(text, r'^(CRATES=")\n(?:\t.*\n)*(?=^"$)', "\n" + crates)


def update_winget(text: str, release: Release) -> str:
    """Updates whichever of version, download, checksum and notes a winget manifest holds."""
    text = set_field(text, r"^(PackageVersion: ).*$", release.version)
    if "InstallerUrl:" in text:
        text = set_field(text, r"^(.*InstallerUrl: .*/download/v)[^/]+", release.version)
        text = set_field(text, r"^(\s*InstallerSha256: ).*$", release.windows_sha256.upper())
    if "ReleaseNotesUrl:" in text:
        text = set_field(text, r"^(ReleaseNotesUrl: .*/tag/v).*$", release.version)
    return text


def update_nuspec(text: str, release: Release) -> str:
    """Sets the version and the release notes link of the Chocolatey package."""
    text = set_field(text, r"^(\s*<version>)[^<]*", release.version)
    return set_field(text, r"^(\s*<releaseNotes>.*/tag/v)[^<]*", release.version)


def update_chocolatey_install(text: str, release: Release) -> str:
    """Sets the download and the checksum of the Windows archive in the install script."""
    text = set_field(text, r"^(\$url64 = '.*/download/v)[^/]+", release.version)
    return set_field(text, r"^(\s*checksum64\s*= ').*$", release.windows_sha256 + "'")


def rewrite(path: Path, update, release: Release) -> None:
    """Applies `update` to the file at `path`."""
    path.write_text(update(path.read_text(encoding="utf-8"), release), encoding="utf-8")


def apply_release(root: Path, release: Release) -> None:
    """Updates every recipe below `root` for `release` and renames the ebuild."""
    rewrite(root / PKGBUILD, update_pkgbuild, release)
    rewrite(root / SRCINFO, update_srcinfo, release)
    rewrite(root / RPM_SPEC, update_rpm_spec, release)
    rewrite(root / NUSPEC, update_nuspec, release)
    rewrite(root / CHOCOLATEY_INSTALL, update_chocolatey_install, release)
    for manifest in sorted((root / WINGET_DIRECTORY).glob("*.yaml")):
        rewrite(manifest, update_winget, release)
    ebuilds = list((root / EBUILD_DIRECTORY).glob("rudo-tui-*.ebuild"))
    if len(ebuilds) != 1:
        raise UpdateError(f"expected one ebuild in {EBUILD_DIRECTORY}, found {len(ebuilds)}")
    rewrite(ebuilds[0], update_ebuild, release)
    ebuilds[0].rename(ebuilds[0].with_name(f"rudo-tui-{release.version}.ebuild"))


def download_sha256(version: str, name: str) -> str:
    """Downloads the `.sha256` file of the release file `name`."""
    url = RELEASE_DOWNLOAD_URL.format(version=version, name=f"{name}.sha256")
    with urllib.request.urlopen(url, timeout=30) as response:
        return parse_sha256_file(response.read().decode("utf-8"))


def read_release(root: Path, version: str) -> Release:
    """Collects the facts of `version` from its GitHub release and the checkout at `root`."""
    if not VERSION_PATTERN.fullmatch(version):
        raise UpdateError(f"not a version: {version!r}")
    cargo_toml = (root / "Cargo.toml").read_text(encoding="utf-8")
    checked_out = tomllib.loads(cargo_toml)["package"]["version"]
    if checked_out != version:
        raise UpdateError(f"Cargo.toml has version {checked_out}; check out the tag v{version}")
    return Release(
        version=version,
        source_sha256=download_sha256(version, SOURCE_ARCHIVE),
        windows_sha256=download_sha256(version, WINDOWS_ARCHIVE),
        rust_version=minimum_rust_version(cargo_toml),
        crates=crate_list((root / "Cargo.lock").read_text(encoding="utf-8")),
        date=datetime.date.today(),
    )


def main(arguments: list[str]) -> int:
    if len(arguments) != 1:
        print(__doc__, file=sys.stderr)
        return 2
    try:
        apply_release(REPOSITORY_ROOT, read_release(REPOSITORY_ROOT, arguments[0]))
    except (UpdateError, OSError) as error:
        print(f"error: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))

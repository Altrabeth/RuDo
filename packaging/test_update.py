"""Tests of packaging/update.py. Run with `just test-packaging`."""

import datetime
import shutil
import tempfile
import unittest
from pathlib import Path

import update

RELEASE = update.Release(
    version="1.2.3",
    source_sha256="a" * 64,
    windows_sha256="b" * 64,
    rust_version="1.101.0",
    crates=["anyhow@1.0.105", "clap@4.6.9"],
    date=datetime.date(2027, 3, 4),
)


class SetFieldTest(unittest.TestCase):
    def test_replaces_the_value_after_the_prefix(self):
        text = "name=old\nversion=0.1.0\n"
        expected = "name=old\nversion=1.2.3\n"

        system_under_test = update.set_field(text, r"^(version=).*$", "1.2.3")

        self.assertEqual(system_under_test, expected)

    def test_rejects_a_pattern_without_a_match(self):
        text = "name=old\n"

        system_under_test = lambda: update.set_field(text, r"^(version=).*$", "1.2.3")

        self.assertRaises(update.UpdateError, system_under_test)

    def test_rejects_a_pattern_with_several_matches(self):
        text = "version=1\nversion=2\n"

        system_under_test = lambda: update.set_field(text, r"^(version=).*$", "1.2.3")

        self.assertRaises(update.UpdateError, system_under_test)


class ParseSha256FileTest(unittest.TestCase):
    def test_returns_the_lowercase_checksum_of_a_sha256sum_line(self):
        text = "AB" * 32 + "  source.tar.gz\n"
        expected = "ab" * 32

        system_under_test = update.parse_sha256_file(text)

        self.assertEqual(system_under_test, expected)

    def test_rejects_text_without_a_checksum(self):
        text = "<html>Not Found</html>"

        system_under_test = lambda: update.parse_sha256_file(text)

        self.assertRaises(update.UpdateError, system_under_test)

    def test_rejects_an_empty_file(self):
        text = ""

        system_under_test = lambda: update.parse_sha256_file(text)

        self.assertRaises(update.UpdateError, system_under_test)


class CrateListTest(unittest.TestCase):
    def test_lists_the_registry_packages_in_lock_order_without_the_local_package(self):
        cargo_lock = (
            "version = 4\n\n"
            '[[package]]\nname = "anyhow"\nversion = "1.0.105"\n'
            'source = "registry+https://github.com/rust-lang/crates.io-index"\n\n'
            '[[package]]\nname = "rudo-tui"\nversion = "1.0.0"\n\n'
            '[[package]]\nname = "syn"\nversion = "2.0.119"\n'
            'source = "registry+https://github.com/rust-lang/crates.io-index"\n\n'
            '[[package]]\nname = "syn"\nversion = "10.0.1"\n'
            'source = "registry+https://github.com/rust-lang/crates.io-index"\n'
        )
        expected = ["anyhow@1.0.105", "syn@2.0.119", "syn@10.0.1"]

        system_under_test = update.crate_list(cargo_lock)

        self.assertEqual(system_under_test, expected)


class MinimumRustVersionTest(unittest.TestCase):
    def test_pads_the_rust_version_to_three_components(self):
        cargo_toml = '[package]\nname = "rudo-tui"\nrust-version = "1.100"\n'
        expected = "1.100.0"

        system_under_test = update.minimum_rust_version(cargo_toml)

        self.assertEqual(system_under_test, expected)


class UpdatePkgbuildTest(unittest.TestCase):
    def test_sets_version_release_and_checksum(self):
        text = "pkgver=1.0.0\npkgrel=3\nsha256sums=('" + "0" * 64 + "')\n"
        expected = "pkgver=1.2.3\npkgrel=1\nsha256sums=('" + "a" * 64 + "')\n"

        system_under_test = update.update_pkgbuild(text, RELEASE)

        self.assertEqual(system_under_test, expected)


class UpdateSrcinfoTest(unittest.TestCase):
    def test_sets_version_release_source_and_checksum(self):
        text = (
            "pkgbase = rudo-tui\n\tpkgver = 1.0.0\n\tpkgrel = 2\n"
            "\tsource = rudo-tui-1.0.0.tar.gz::https://example.invalid/source.tar.gz\n"
            "\tsha256sums = SKIP\n"
        )
        expected = (
            "pkgbase = rudo-tui\n\tpkgver = 1.2.3\n\tpkgrel = 1\n"
            "\tsource = rudo-tui-1.2.3.tar.gz::"
            "https://github.com/Altrabeth/RuDo/releases/download/v1.2.3/source.tar.gz\n"
            f"\tsha256sums = {'a' * 64}\n"
        )

        system_under_test = update.update_srcinfo(text, RELEASE)

        self.assertEqual(system_under_test, expected)


class UpdateRpmSpecTest(unittest.TestCase):
    def test_sets_version_and_rust_and_adds_a_changelog_entry_by_the_last_author(self):
        text = (
            "Version:        1.0.0\nRelease:        4%{?dist}\n"
            "BuildRequires:  cargo >= 1.100\nBuildRequires:  rust >= 1.100\n"
            "%changelog\n* Wed Nov 25 2026 A Packager <a@example.org> - 1.0.0-1\n- First\n"
        )
        expected = (
            "Version:        1.2.3\nRelease:        1%{?dist}\n"
            "BuildRequires:  cargo >= 1.101\nBuildRequires:  rust >= 1.101\n"
            "%changelog\n* Thu Mar 04 2027 A Packager <a@example.org> - 1.2.3-1\n"
            "- Update to 1.2.3\n\n"
            "* Wed Nov 25 2026 A Packager <a@example.org> - 1.0.0-1\n- First\n"
        )

        system_under_test = update.update_rpm_spec(text, RELEASE)

        self.assertEqual(system_under_test, expected)

    def test_rejects_a_spec_without_changelog_entry(self):
        text = (
            "Version: 1.0.0\nRelease: 1\nBuildRequires: cargo >= 1.100\n"
            "BuildRequires: rust >= 1.100\n%changelog\n"
        )

        system_under_test = lambda: update.update_rpm_spec(text, RELEASE)

        self.assertRaises(update.UpdateError, system_under_test)


class UpdateEbuildTest(unittest.TestCase):
    def test_replaces_the_crate_list_and_the_minimum_rust(self):
        text = 'CRATES="\n\told@0.1.0\n"\nRUST_MIN_VER="1.100.0"\ninherit cargo\n'
        expected = (
            'CRATES="\n\tanyhow@1.0.105\n\tclap@4.6.9\n"\nRUST_MIN_VER="1.101.0"\ninherit cargo\n'
        )

        system_under_test = update.update_ebuild(text, RELEASE)

        self.assertEqual(system_under_test, expected)

    def test_rejects_an_ebuild_without_crate_list(self):
        text = 'RUST_MIN_VER="1.100.0"\ninherit cargo\n'

        system_under_test = lambda: update.update_ebuild(text, RELEASE)

        self.assertRaises(update.UpdateError, system_under_test)


class UpdateWingetTest(unittest.TestCase):
    def test_sets_version_download_and_uppercase_checksum_of_the_installer(self):
        text = (
            "PackageVersion: 1.0.0\nInstallers:\n"
            "  - InstallerUrl: https://github.com/Altrabeth/RuDo/releases/download/v1.0.0/"
            "rudo-tui-x86_64-pc-windows-msvc.zip\n"
            f"    InstallerSha256: {'0' * 64}\n"
        )
        expected = (
            "PackageVersion: 1.2.3\nInstallers:\n"
            "  - InstallerUrl: https://github.com/Altrabeth/RuDo/releases/download/v1.2.3/"
            "rudo-tui-x86_64-pc-windows-msvc.zip\n"
            f"    InstallerSha256: {'B' * 64}\n"
        )

        system_under_test = update.update_winget(text, RELEASE)

        self.assertEqual(system_under_test, expected)

    def test_sets_version_and_release_notes_of_the_locale(self):
        text = (
            "PackageVersion: 1.0.0\n"
            "ReleaseNotesUrl: https://github.com/Altrabeth/RuDo/releases/tag/v1.0.0\n"
        )
        expected = (
            "PackageVersion: 1.2.3\n"
            "ReleaseNotesUrl: https://github.com/Altrabeth/RuDo/releases/tag/v1.2.3\n"
        )

        system_under_test = update.update_winget(text, RELEASE)

        self.assertEqual(system_under_test, expected)


class UpdateChocolateyTest(unittest.TestCase):
    def test_sets_version_and_release_notes_of_the_nuspec(self):
        text = (
            "    <version>1.0.0</version>\n"
            "    <releaseNotes>https://github.com/Altrabeth/RuDo/releases/tag/v1.0.0"
            "</releaseNotes>\n"
        )
        expected = (
            "    <version>1.2.3</version>\n"
            "    <releaseNotes>https://github.com/Altrabeth/RuDo/releases/tag/v1.2.3"
            "</releaseNotes>\n"
        )

        system_under_test = update.update_nuspec(text, RELEASE)

        self.assertEqual(system_under_test, expected)

    def test_sets_download_and_checksum_of_the_install_script(self):
        text = (
            "$url64 = 'https://github.com/Altrabeth/RuDo/releases/download/v1.0.0/x.zip'\n"
            f"  checksum64     = '{'0' * 64}'\n"
        )
        expected = (
            "$url64 = 'https://github.com/Altrabeth/RuDo/releases/download/v1.2.3/x.zip'\n"
            f"  checksum64     = '{'b' * 64}'\n"
        )

        system_under_test = update.update_chocolatey_install(text, RELEASE)

        self.assertEqual(system_under_test, expected)


class ApplyReleaseTest(unittest.TestCase):
    """Runs the update on a copy of the recipes of this repository."""

    def setUp(self):
        self.root = Path(tempfile.mkdtemp())
        self.addCleanup(shutil.rmtree, self.root)
        shutil.copytree(update.REPOSITORY_ROOT / "packaging", self.root / "packaging")

    def test_updates_every_recipe_and_renames_the_ebuild(self):
        ebuild = self.root / update.EBUILD_DIRECTORY / "rudo-tui-1.2.3.ebuild"
        expected_lines = {
            self.root / update.PKGBUILD: "pkgver=1.2.3",
            self.root / update.SRCINFO: f"\tsha256sums = {'a' * 64}",
            self.root / update.RPM_SPEC: "- Update to 1.2.3",
            ebuild: "\tclap@4.6.9",
            self.root / update.WINGET_DIRECTORY / "Altrabeth.RuDo.yaml": "PackageVersion: 1.2.3",
            self.root / update.WINGET_DIRECTORY / "Altrabeth.RuDo.installer.yaml": (
                f"    InstallerSha256: {'B' * 64}"
            ),
            self.root / update.WINGET_DIRECTORY / "Altrabeth.RuDo.locale.en-US.yaml": (
                "PackageVersion: 1.2.3"
            ),
            self.root / update.NUSPEC: "    <version>1.2.3</version>",
            self.root / update.CHOCOLATEY_INSTALL: f"  checksum64     = '{'b' * 64}'",
        }

        update.apply_release(self.root, RELEASE)
        system_under_test = {path: path.read_text().splitlines() for path in expected_lines}

        for path, line in expected_lines.items():
            self.assertIn(line, system_under_test[path], path)
        self.assertEqual(len(list(ebuild.parent.glob("*.ebuild"))), 1)


class ReadReleaseTest(unittest.TestCase):
    def test_rejects_a_version_that_is_not_checked_out(self):
        version = "0.0.1"

        system_under_test = lambda: update.read_release(update.REPOSITORY_ROOT, version)

        self.assertRaisesRegex(update.UpdateError, "check out the tag v0.0.1", system_under_test)

    def test_rejects_text_that_is_not_a_version(self):
        version = "latest"

        system_under_test = lambda: update.read_release(update.REPOSITORY_ROOT, version)

        self.assertRaisesRegex(update.UpdateError, "not a version", system_under_test)


if __name__ == "__main__":
    unittest.main()

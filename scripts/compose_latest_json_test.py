#!/usr/bin/env python3
"""Fixture tests for compose_latest_json.py.

Builds a synthetic artifact tree shaped exactly like the desktop release
pipeline's downloaded artifacts (one desktop-<os>-<arch> directory per matrix
leg, bundle files + base64 minisign .sig siblings) and asserts the composed
manifest matches the schema tauri-plugin-updater 2.10.1 deserializes
(RemoteRelease: version/notes/pub_date/platforms.<key>.{signature,url}).

Run: python3 scripts/compose_latest_json_test.py
"""

from __future__ import annotations

import base64
import json
import sys
import tempfile
import unittest
from datetime import datetime
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

import compose_latest_json  # noqa: E402


def fake_sig(name: str) -> str:
    """Base64 minisign signature box, as tauri's updater signing emits."""
    box = (
        "untrusted comment: signature from tauri secret key\n"
        "RUTFAKESIGFAKESIGFAKESIG\n"
        f"trusted comment: timestamp:1753660800\tfile:{name}\n"
        "RUTFAKEGLOBALSIGFAKEGLOBALSIG\n"
    )
    return base64.standard_b64encode(box.encode("ascii")).decode("ascii")


def write_bundle(directory: Path, name: str, signed: bool = True) -> None:
    directory.mkdir(parents=True, exist_ok=True)
    (directory / name).write_bytes(b"fake bundle bytes for " + name.encode())
    if signed:
        (directory / (name + ".sig")).write_text(fake_sig(name), encoding="ascii")


BASE_URL = "https://github.com/hairbui76/RustlingPDF/releases/download/v2.14.2"


class ComposeLatestJsonTest(unittest.TestCase):
    def setUp(self) -> None:
        self._tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self._tmp.cleanup)
        self.artifacts = Path(self._tmp.name) / "desktop-artifacts"

    def stage_default_tree(self) -> None:
        """The exact artifact layout the three release matrix legs upload."""
        linux = self.artifacts / "desktop-linux-x86_64"
        write_bundle(linux, "Stirling.PDF_2.14.2_amd64.AppImage")
        write_bundle(linux, "stirling-pdf_2.14.2_amd64.deb")

        windows = self.artifacts / "desktop-windows-x86_64"
        write_bundle(windows, "Stirling.PDF_2.14.2_x64_en-US.msi")

        darwin = self.artifacts / "desktop-darwin-aarch64"
        write_bundle(darwin, "Stirling.PDF.app.tar.gz")
        # The dmg is uploaded for humans; it is not an updater artifact and
        # carries no signature — the composer must ignore it.
        write_bundle(darwin, "Stirling.PDF_2.14.2_aarch64.dmg", signed=False)

    def compose(self, **overrides):
        kwargs = dict(
            artifacts_dir=self.artifacts,
            version="2.14.2",
            notes="RustlingPDF v2.14.2",
            base_url=BASE_URL,
            pub_date="2026-07-28T00:00:00Z",
        )
        kwargs.update(overrides)
        return compose_latest_json.compose(**kwargs)

    def test_composes_all_three_platforms(self) -> None:
        self.stage_default_tree()
        manifest = self.compose()

        self.assertEqual(manifest["version"], "2.14.2")
        self.assertEqual(manifest["notes"], "RustlingPDF v2.14.2")
        self.assertEqual(manifest["pub_date"], "2026-07-28T00:00:00Z")
        self.assertEqual(
            sorted(manifest["platforms"]),
            [
                "darwin-aarch64",
                "linux-x86_64",
                "linux-x86_64-deb",
                "windows-x86_64",
                "windows-x86_64-msi",
            ],
        )
        self.assertEqual(
            manifest["platforms"]["linux-x86_64"]["url"],
            f"{BASE_URL}/Stirling.PDF_2.14.2_amd64.AppImage",
        )
        self.assertEqual(
            manifest["platforms"]["linux-x86_64-deb"]["url"],
            f"{BASE_URL}/stirling-pdf_2.14.2_amd64.deb",
        )
        self.assertEqual(
            manifest["platforms"]["windows-x86_64"]["url"],
            f"{BASE_URL}/Stirling.PDF_2.14.2_x64_en-US.msi",
        )
        self.assertEqual(
            manifest["platforms"]["windows-x86_64"],
            manifest["platforms"]["windows-x86_64-msi"],
        )
        self.assertEqual(
            manifest["platforms"]["darwin-aarch64"]["url"],
            f"{BASE_URL}/Stirling.PDF.app.tar.gz",
        )

        # Signature fields are the verbatim .sig contents.
        appimage_sig = (
            self.artifacts
            / "desktop-linux-x86_64"
            / "Stirling.PDF_2.14.2_amd64.AppImage.sig"
        ).read_text(encoding="ascii")
        self.assertEqual(
            manifest["platforms"]["linux-x86_64"]["signature"], appimage_sig
        )

    def test_manifest_matches_remote_release_deserialization(self) -> None:
        """Re-implement the plugin's RemoteRelease parse checks in Python."""
        self.stage_default_tree()
        manifest = json.loads(json.dumps(self.compose()))

        # parse_version: trims a leading 'v', requires semver.
        self.assertRegex(manifest["version"].lstrip("v"), r"^\d+\.\d+\.\d+$")
        # pub_date: RFC 3339.
        datetime.fromisoformat(manifest["pub_date"].replace("Z", "+00:00"))
        # Static shape: platforms map present, every entry has url + signature.
        self.assertIsInstance(manifest["platforms"], dict)
        self.assertGreater(len(manifest["platforms"]), 0)
        for key, entry in manifest["platforms"].items():
            self.assertEqual(sorted(entry), ["signature", "url"])
            self.assertTrue(entry["url"].startswith("https://"))
            self.assertNotIn(" ", entry["url"])
            # verify_signature: base64 → minisign signature box.
            decoded = base64.standard_b64decode(entry["signature"])
            self.assertTrue(decoded.startswith(b"untrusted comment:"), key)

    def test_nsis_preferred_for_bare_windows_key(self) -> None:
        self.stage_default_tree()
        windows = self.artifacts / "desktop-windows-x86_64"
        write_bundle(windows, "Stirling.PDF_2.14.2_x64-setup.exe")
        manifest = self.compose()
        self.assertEqual(
            manifest["platforms"]["windows-x86_64"]["url"],
            f"{BASE_URL}/Stirling.PDF_2.14.2_x64-setup.exe",
        )
        self.assertEqual(
            manifest["platforms"]["windows-x86_64-msi"]["url"],
            f"{BASE_URL}/Stirling.PDF_2.14.2_x64_en-US.msi",
        )
        self.assertEqual(
            manifest["platforms"]["windows-x86_64-nsis"]["url"],
            f"{BASE_URL}/Stirling.PDF_2.14.2_x64-setup.exe",
        )

    def test_strips_leading_v_from_version(self) -> None:
        self.stage_default_tree()
        self.assertEqual(self.compose(version="v2.14.2")["version"], "2.14.2")

    def test_default_pub_date_is_rfc3339_utc(self) -> None:
        self.stage_default_tree()
        pub_date = self.compose(pub_date=None)["pub_date"]
        self.assertTrue(pub_date.endswith("Z"))
        datetime.fromisoformat(pub_date.replace("Z", "+00:00"))

    def assert_compose_error(self, fragment: str, **overrides) -> None:
        with self.assertRaises(compose_latest_json.ComposeError) as ctx:
            self.compose(**overrides)
        self.assertIn(fragment, str(ctx.exception))

    def test_rejects_missing_signature(self) -> None:
        self.stage_default_tree()
        (
            self.artifacts
            / "desktop-linux-x86_64"
            / "Stirling.PDF_2.14.2_amd64.AppImage.sig"
        ).unlink()
        self.assert_compose_error("missing updater signature")

    def test_rejects_non_base64_signature(self) -> None:
        self.stage_default_tree()
        (
            self.artifacts
            / "desktop-darwin-aarch64"
            / "Stirling.PDF.app.tar.gz.sig"
        ).write_text("not base64 at all!!", encoding="ascii")
        self.assert_compose_error("not valid base64")

    def test_rejects_non_minisign_signature(self) -> None:
        self.stage_default_tree()
        bogus = base64.standard_b64encode(b"something else entirely").decode("ascii")
        (
            self.artifacts
            / "desktop-darwin-aarch64"
            / "Stirling.PDF.app.tar.gz.sig"
        ).write_text(bogus, encoding="ascii")
        self.assert_compose_error("minisign")

    def test_rejects_space_in_artifact_name(self) -> None:
        self.stage_default_tree()
        linux = self.artifacts / "desktop-linux-x86_64"
        (linux / "stirling-pdf_2.14.2_amd64.deb").rename(
            linux / "stirling pdf_2.14.2_amd64.deb"
        )
        (linux / "stirling-pdf_2.14.2_amd64.deb.sig").rename(
            linux / "stirling pdf_2.14.2_amd64.deb.sig"
        )
        self.assert_compose_error("space")

    def test_rejects_two_appimages(self) -> None:
        self.stage_default_tree()
        write_bundle(
            self.artifacts / "desktop-linux-x86_64",
            "Stirling.PDF_9.9.9_amd64.AppImage",
        )
        self.assert_compose_error("exactly one")

    def test_rejects_missing_base_artifact(self) -> None:
        self.stage_default_tree()
        linux = self.artifacts / "desktop-linux-x86_64"
        (linux / "Stirling.PDF_2.14.2_amd64.AppImage").unlink()
        (linux / "Stirling.PDF_2.14.2_amd64.AppImage.sig").unlink()
        self.assert_compose_error("no *.AppImage found")

    def test_rejects_unknown_os_directory(self) -> None:
        self.stage_default_tree()
        write_bundle(self.artifacts / "desktop-beos-x86_64", "x.AppImage")
        self.assert_compose_error("unknown updater os")

    def test_rejects_unknown_arch_directory(self) -> None:
        self.stage_default_tree()
        write_bundle(self.artifacts / "desktop-linux-mips64", "x.AppImage")
        self.assert_compose_error("unknown updater arch")

    def test_rejects_stray_directory_name(self) -> None:
        self.stage_default_tree()
        (self.artifacts / "not-a-desktop-artifact").mkdir()
        self.assert_compose_error("unexpected entry")

    def test_rejects_empty_artifacts_dir(self) -> None:
        self.artifacts.mkdir(parents=True)
        self.assert_compose_error("no desktop-<os>-<arch>")

    def test_rejects_suffixed_version(self) -> None:
        self.stage_default_tree()
        self.assert_compose_error("X.Y.Z", version="2.14.2-rc1")

    def test_rejects_bad_pub_date(self) -> None:
        self.stage_default_tree()
        self.assert_compose_error("RFC 3339", pub_date="July 28th 2026")

    def test_cli_end_to_end(self) -> None:
        self.stage_default_tree()
        output = Path(self._tmp.name) / "latest.json"
        exit_code = compose_latest_json.main(
            [
                "--artifacts-dir",
                str(self.artifacts),
                "--version",
                "2.14.2",
                "--notes",
                "notes",
                "--base-url",
                BASE_URL,
                "--pub-date",
                "2026-07-28T00:00:00Z",
                "--output",
                str(output),
            ]
        )
        self.assertEqual(exit_code, 0)
        manifest = json.loads(output.read_text(encoding="utf-8"))
        self.assertEqual(manifest["version"], "2.14.2")
        self.assertIn("linux-x86_64", manifest["platforms"])

    def test_cli_failure_exit_code(self) -> None:
        output = Path(self._tmp.name) / "latest.json"
        exit_code = compose_latest_json.main(
            [
                "--artifacts-dir",
                str(self.artifacts / "missing"),
                "--version",
                "2.14.2",
                "--base-url",
                BASE_URL,
                "--output",
                str(output),
            ]
        )
        self.assertEqual(exit_code, 1)
        self.assertFalse(output.exists())


if __name__ == "__main__":
    unittest.main(verbosity=2)

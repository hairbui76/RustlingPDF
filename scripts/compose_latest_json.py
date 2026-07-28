#!/usr/bin/env python3
"""Compose the Tauri updater manifest (latest.json) from desktop build artifacts.

The desktop release pipeline uploads one artifact directory per matrix leg,
named ``desktop-<os>-<arch>`` (e.g. ``desktop-linux-x86_64``), each containing
the bundles produced by ``tauri build`` plus the ``.sig`` files emitted by the
updater signing step. This script walks those directories and emits the static
manifest that tauri-plugin-updater polls.

Schema (derived from tauri-plugin-updater 2.10.1 ``src/updater.rs`` — the
version bundled with this repo's Tauri shell):

- Top level: ``version`` (``v`` prefix tolerated by the plugin, emitted bare),
  ``notes``, ``pub_date`` (RFC 3339), ``platforms`` (map).
- Platform keys: the plugin looks up ``{os}-{arch}-{installer}`` first (when
  it knows the installed bundle type) and falls back to ``{os}-{arch}``, where
  os is ``linux`` / ``darwin`` / ``windows`` and arch is ``x86_64`` /
  ``aarch64`` / ``i686`` / ``armv7`` (``updater_os()`` / ``updater_arch()``).
- Each platform entry: ``url`` (download URL) and ``signature`` — the verbatim
  content of the ``.sig`` file, which is base64 over the minisign signature
  box (``verify_signature()`` base64-decodes it and parses the box).

Which artifact each platform key points at (``createUpdaterArtifacts: true``,
i.e. NOT v1-compatible — from tauri-bundler 2.x ``bundle.rs`` +
``sign_updaters`` in tauri-cli ``bundle.rs``):

- linux: the ``.AppImage`` itself is signed and served (the plugin's
  ``install_appimage`` accepts raw AppImage bytes). ``.deb`` / ``.rpm`` are
  also signed when built; they are published under the installer-specific
  keys (``linux-<arch>-deb`` / ``linux-<arch>-rpm``) so deb/rpm installs
  update with matching package bytes (``install_deb`` / ``install_rpm``
  reject anything else).
- darwin: the ``.app.tar.gz`` produced by the bundler's updater step (the
  macOS installer path only understands the gzipped app bundle).
- windows: the ``.msi`` (WiX) and/or ``-setup.exe`` (NSIS) installer itself.
  The NSIS installer is preferred for the bare ``windows-<arch>`` key when
  both exist (matching tauri-action), and both get their installer-specific
  keys.

Artifact file names must not contain spaces: GitHub release assets rename
``' '`` to ``'.'`` on upload, which would silently break every composed URL.
The build workflow renames bundles the same way before uploading; this script
fails hard if a space slips through.
"""

from __future__ import annotations

import argparse
import base64
import binascii
import json
import re
import sys
from datetime import datetime, timezone
from pathlib import Path

KNOWN_OS = {"linux", "darwin", "windows"}
KNOWN_ARCH = {"x86_64", "aarch64", "i686", "armv7"}

ARTIFACT_DIR_RE = re.compile(r"^desktop-(?P<os>[a-z]+)-(?P<arch>[a-z0-9_]+)$")
VERSION_RE = re.compile(r"^\d+\.\d+\.\d+$")


class ComposeError(Exception):
    """Raised for any manifest composition failure (exit code 1)."""


def _fail(message: str) -> "ComposeError":
    return ComposeError(message)


def read_signature(artifact: Path) -> str:
    """Read and validate the .sig sibling of an updater artifact.

    The signature field the plugin verifies is the literal .sig file content:
    base64 (standard alphabet) over the minisign signature box, whose decoded
    text starts with ``untrusted comment:``.
    """
    sig_path = artifact.with_name(artifact.name + ".sig")
    if not sig_path.is_file():
        raise _fail(f"missing updater signature: {sig_path}")
    signature = sig_path.read_text(encoding="ascii").strip()
    if not signature:
        raise _fail(f"empty updater signature: {sig_path}")
    try:
        decoded = base64.standard_b64decode(signature)
    except (binascii.Error, ValueError) as error:
        raise _fail(f"signature is not valid base64: {sig_path}: {error}") from error
    if not decoded.startswith(b"untrusted comment:"):
        raise _fail(
            f"decoded signature is not a minisign signature box: {sig_path}"
        )
    return signature


def artifact_url(base_url: str, artifact: Path) -> str:
    if " " in artifact.name:
        raise _fail(
            f"artifact file name contains a space (GitHub would rename it to '.' "
            f"on upload and break the composed URL): {artifact.name}"
        )
    return f"{base_url.rstrip('/')}/{artifact.name}"


def _single(paths: list[Path], what: str, directory: Path) -> Path:
    if not paths:
        raise _fail(f"no {what} found in {directory}")
    if len(paths) > 1:
        names = ", ".join(sorted(p.name for p in paths))
        raise _fail(f"expected exactly one {what} in {directory}, found: {names}")
    return paths[0]


def platform_entries(directory: Path, os_name: str, arch: str, base_url: str) -> dict:
    """Derive the platform-key → {signature, url} entries for one artifact dir."""

    def bundles(suffix: str) -> list[Path]:
        return sorted(
            p
            for p in directory.iterdir()
            if p.is_file() and p.name.endswith(suffix) and not p.name.endswith(".sig")
        )

    def entry(artifact: Path) -> dict:
        return {
            "signature": read_signature(artifact),
            "url": artifact_url(base_url, artifact),
        }

    base_key = f"{os_name}-{arch}"
    entries: dict[str, dict] = {}

    if os_name == "linux":
        appimage = _single(bundles(".AppImage"), "*.AppImage", directory)
        entries[base_key] = entry(appimage)
        for suffix, installer in ((".deb", "deb"), (".rpm", "rpm")):
            packages = bundles(suffix)
            if packages:
                package = _single(packages, f"*{suffix}", directory)
                entries[f"{base_key}-{installer}"] = entry(package)
    elif os_name == "darwin":
        app_archive = _single(bundles(".app.tar.gz"), "*.app.tar.gz", directory)
        entries[base_key] = entry(app_archive)
    elif os_name == "windows":
        nsis_installers = bundles("-setup.exe")
        msi_installers = bundles(".msi")
        if not nsis_installers and not msi_installers:
            raise _fail(f"no *.msi or *-setup.exe installer found in {directory}")
        if nsis_installers:
            nsis = _single(nsis_installers, "*-setup.exe", directory)
            entries[f"{base_key}-nsis"] = entry(nsis)
        if msi_installers:
            msi = _single(msi_installers, "*.msi", directory)
            entries[f"{base_key}-msi"] = entry(msi)
        # Bare-key fallback: NSIS preferred when both exist (tauri-action rule).
        preferred = nsis_installers[0] if nsis_installers else msi_installers[0]
        entries[base_key] = entry(preferred)
    else:  # pragma: no cover - guarded by KNOWN_OS check in compose()
        raise _fail(f"unsupported updater os '{os_name}' for {directory}")

    return entries


def compose(
    artifacts_dir: Path,
    version: str,
    notes: str,
    base_url: str,
    pub_date: str | None,
) -> dict:
    version = version.lstrip("v")
    if not VERSION_RE.match(version):
        raise _fail(
            f"version '{version}' is not a plain X.Y.Z version "
            "(the release pipeline publishes exact releases only)"
        )

    if pub_date is None:
        pub_date = (
            datetime.now(timezone.utc).replace(microsecond=0).isoformat()
        ).replace("+00:00", "Z")
    # The plugin parses pub_date with time's RFC 3339 well-known format.
    try:
        datetime.fromisoformat(pub_date.replace("Z", "+00:00"))
    except ValueError as error:
        raise _fail(f"pub_date '{pub_date}' is not RFC 3339: {error}") from error

    if not artifacts_dir.is_dir():
        raise _fail(f"artifacts directory not found: {artifacts_dir}")

    platforms: dict[str, dict] = {}
    for directory in sorted(artifacts_dir.iterdir()):
        if not directory.is_dir():
            continue
        match = ARTIFACT_DIR_RE.match(directory.name)
        if not match:
            raise _fail(
                f"unexpected entry in artifacts directory (want desktop-<os>-<arch> "
                f"directories only): {directory.name}"
            )
        os_name = match.group("os")
        arch = match.group("arch")
        if os_name not in KNOWN_OS:
            raise _fail(
                f"unknown updater os '{os_name}' in {directory.name} "
                f"(known: {sorted(KNOWN_OS)})"
            )
        if arch not in KNOWN_ARCH:
            raise _fail(
                f"unknown updater arch '{arch}' in {directory.name} "
                f"(known: {sorted(KNOWN_ARCH)})"
            )
        for key, value in platform_entries(directory, os_name, arch, base_url).items():
            if key in platforms:
                raise _fail(f"duplicate platform key '{key}' (from {directory.name})")
            platforms[key] = value

    if not platforms:
        raise _fail(f"no desktop-<os>-<arch> artifact directories in {artifacts_dir}")

    return {
        "version": version,
        "notes": notes,
        "pub_date": pub_date,
        "platforms": platforms,
    }


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument(
        "--artifacts-dir",
        required=True,
        type=Path,
        help="directory containing one desktop-<os>-<arch> subdirectory per matrix leg",
    )
    parser.add_argument("--version", required=True, help="release version (X.Y.Z; a leading v is stripped)")
    parser.add_argument("--notes", default="", help="release notes line for the manifest")
    parser.add_argument(
        "--base-url",
        required=True,
        help="download URL prefix the artifact file names are appended to "
        "(e.g. https://github.com/<owner>/<repo>/releases/download/vX.Y.Z)",
    )
    parser.add_argument(
        "--pub-date",
        default=None,
        help="RFC 3339 publication timestamp (default: now, UTC)",
    )
    parser.add_argument(
        "--output",
        required=True,
        type=Path,
        help="path to write latest.json to",
    )
    args = parser.parse_args(argv)

    try:
        manifest = compose(
            artifacts_dir=args.artifacts_dir,
            version=args.version,
            notes=args.notes,
            base_url=args.base_url,
            pub_date=args.pub_date,
        )
    except ComposeError as error:
        print(f"compose_latest_json: error: {error}", file=sys.stderr)
        return 1

    args.output.write_text(
        json.dumps(manifest, indent=2, sort_keys=False) + "\n", encoding="utf-8"
    )
    print(
        f"compose_latest_json: wrote {args.output} "
        f"({len(manifest['platforms'])} platform keys: "
        f"{', '.join(sorted(manifest['platforms']))})"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())

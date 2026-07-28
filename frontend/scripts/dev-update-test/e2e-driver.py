#!/usr/bin/env python3
"""Signed-upgrade e2e driver (Linux leg).

Launches the v0.0.1 AppImage under the current DISPLAY (Xvfb), attaches to
the WebKit remote inspector, and drives the Tauri updater IPC commands:

  T1  get_app_version            == 0.0.1
  T2  check_for_update           offers 99.0.0 (from the served latest.json)
  T3  download_and_install_update with a wrong-key signature   -> REJECTED
  T4  download_and_install_update with tampered artifact bytes -> REJECTED
  T5  download_and_install_update with the good manifest       -> installs,
      the AppImage on disk is byte-replaced with the v99 artifact
  T6  relaunch the replaced AppImage -> get_app_version == 99.0.0

T3-T6 only run with --install. Assertion results and raw evidence are
written to <work-dir>/evidence/.

Driving protocol: WebKitGTK >= 2.40 exposes the remote inspector over HTTP
(WEBKIT_INSPECTOR_HTTP_SERVER): GET / lists inspectable targets, and a
WebSocket per target speaks the raw WebKit inspector protocol (JSON-RPC,
Runtime.evaluate etc.). A CDP flavor (/json + webSocketDebuggerUrl) is also
attempted first so the same driver can serve the Windows WebView2 leg.

Async IPC results are transported by polling: an evaluate() kicks off the
invoke and stores the outcome on window.__E2E_RESULT, which we poll with
plain synchronous Runtime.evaluate — this avoids depending on either
protocol's promise-awaiting quirks.
"""

import argparse
import asyncio
import hashlib
import json
import os
import re
import shutil
import signal
import subprocess
import sys
import tarfile
import time
import urllib.request

import websockets

DEBUG = os.environ.get("E2E_DRIVER_DEBUG", "") == "1"


def log(msg):
    print(msg, flush=True)


def dbg(msg):
    if DEBUG:
        print(f"    [debug] {msg}", flush=True)


def sha256(path):
    h = hashlib.sha256()
    with open(path, "rb") as f:
        for chunk in iter(lambda: f.read(1 << 20), b""):
            h.update(chunk)
    return h.hexdigest()


def http_get(url, timeout=3):
    with urllib.request.urlopen(url, timeout=timeout) as r:
        return r.read().decode("utf-8", "replace")


# ── inspector attachment ─────────────────────────────────────────────────────


class Inspector:
    """Adapter over the two remote-debugging flavors.

    Exposes a single primitive: eval_sync(expression) -> str value of a
    synchronous JS expression evaluated in the page.
    """

    def __init__(self, ws, flavor):
        self.ws = ws
        self.flavor = flavor
        self._id = 100

    @classmethod
    async def attach(cls, port, timeout=180):
        deadline = time.time() + timeout
        last_err = None
        while time.time() < deadline:
            try:
                return await cls._try_attach(port)
            except Exception as e:  # noqa: BLE001 - retried until deadline
                last_err = e
                dbg(f"attach attempt failed: {e!r}")
                await asyncio.sleep(2)
        raise TimeoutError(f"Could not attach to inspector on :{port}: {last_err!r}")

    @classmethod
    async def _try_attach(cls, port):
        base = f"http://127.0.0.1:{port}"
        # CDP flavor (WebView2 / chromium-style): /json target list.
        try:
            targets = json.loads(http_get(f"{base}/json"))
            page = next(t for t in targets if t.get("type") == "page")
            ws = await websockets.connect(page["webSocketDebuggerUrl"], max_size=50 * 1024 * 1024)
            insp = cls(ws, "cdp")
            await insp._send({"id": 1, "method": "Runtime.enable"})
            await insp._drain()
            log(f"  Attached (CDP): {page.get('url', '?')}")
            return insp
        except Exception as e:  # noqa: BLE001 - fall through to WebKit flavor
            dbg(f"CDP flavor unavailable: {e!r}")

        # WebKitGTK HTTP remote inspector: parse the target list page for the
        # per-target WebSocket path. WebKit 2.5x links each target as
        # Main.html?ws=<host>/socket/<connectionId>/<targetId>/<targetType>.
        html = http_get(f"{base}/")
        dbg(f"target list page:\n{html[:2000]}")
        m = re.search(r"ws='?\s*\+?[^\"']*?(/socket/\d+/\d+(?:/[A-Za-z]+)?)", html) or re.search(
            r"(/socket/\d+/\d+(?:/[A-Za-z]+)?)", html
        )
        if not m:
            raise RuntimeError(f"no inspectable target found on {base}/ (page: {html[:500]!r})")
        ws_path = m.group(1)
        ws_url = ws_path if ws_path.startswith("ws") else f"ws://127.0.0.1:{port}{ws_path}"
        if not ws_url.startswith("ws"):
            ws_url = f"ws://{ws_url}"
        dbg(f"connecting {ws_url}")
        ws = await websockets.connect(ws_url, max_size=50 * 1024 * 1024)
        insp = cls(ws, "webkit")
        await insp._send({"id": 1, "method": "Runtime.enable"})
        await insp._drain()
        log(f"  Attached (WebKit remote inspector): {ws_url}")
        return insp

    async def _send(self, msg):
        await self.ws.send(json.dumps(msg))

    async def _drain(self, quiet=0.3):
        while True:
            try:
                msg = await asyncio.wait_for(self.ws.recv(), timeout=quiet)
                dbg(f"drained: {str(msg)[:300]}")
            except asyncio.TimeoutError:
                return

    async def eval_sync(self, expression, timeout=30):
        """Evaluate a synchronous expression; returns its string/JSON value."""
        self._id += 1
        mid = self._id
        await self._send(
            {
                "id": mid,
                "method": "Runtime.evaluate",
                "params": {"expression": expression, "returnByValue": True},
            }
        )
        deadline = time.time() + timeout
        while True:
            remaining = deadline - time.time()
            if remaining <= 0:
                raise TimeoutError(f"evaluate timed out: {expression[:120]}")
            msg = json.loads(await asyncio.wait_for(self.ws.recv(), timeout=remaining))
            if msg.get("id") != mid:
                dbg(f"skipped event: {str(msg)[:200]}")
                continue
            if "error" in msg:
                raise RuntimeError(f"protocol error: {msg['error']}")
            result = msg.get("result", {})
            if result.get("wasThrown") or "exceptionDetails" in result:
                exc = result.get("result", {}).get("description") or str(
                    result.get("exceptionDetails", "")
                )
                raise RuntimeError(f"JS exception: {exc[:400]}")
            inner = result.get("result", {})
            if "value" in inner:
                return inner["value"]
            return inner.get("description", str(inner))

    async def invoke_async(self, invoke_js, timeout=300, poll=1.0):
        """Run an async IPC invoke and poll for its stored outcome.

        Returns (status, value) where status is 'ok' or 'err'.
        """
        kickoff = (
            "(function(){"
            "window.__E2E_RESULT = {status: 'pending'};"
            f"Promise.resolve().then(function(){{ return {invoke_js}; }})"
            ".then(function(r){ window.__E2E_RESULT = {status:'ok', value: JSON.stringify(r === undefined ? null : r)}; })"
            ".catch(function(e){ window.__E2E_RESULT = {status:'err', value: String(e && e.message ? e.message : e)}; });"
            "return 'started';})()"
        )
        started = await self.eval_sync(kickoff)
        if started != "started":
            raise RuntimeError(f"kickoff failed: {started!r}")
        deadline = time.time() + timeout
        while time.time() < deadline:
            raw = await self.eval_sync("JSON.stringify(window.__E2E_RESULT)")
            state = json.loads(raw)
            if state.get("status") != "pending":
                return state["status"], state.get("value")
            await asyncio.sleep(poll)
        raise TimeoutError(f"async invoke timed out: {invoke_js[:120]}")

    async def close(self):
        try:
            await self.ws.close()
        except Exception:  # noqa: BLE001
            pass


# ── app lifecycle ────────────────────────────────────────────────────────────


class App:
    def __init__(self, appimage, log_path, inspector_port):
        self.appimage = appimage
        self.log_path = log_path
        self.inspector_port = inspector_port
        self.proc = None

    def launch(self):
        env = dict(os.environ)
        env.update(
            {
                # AppImage runtime: no FUSE in the container.
                "APPIMAGE_EXTRACT_AND_RUN": "1",
                # WebKitGTK remote inspector (HTTP flavor; >= 2.40).
                "WEBKIT_INSPECTOR_HTTP_SERVER": f"127.0.0.1:{self.inspector_port}",
                "WEBKIT_INSPECTOR_SERVER": f"127.0.0.1:{self.inspector_port + 1}",
                # Container/headless rendering knobs.
                "WEBKIT_DISABLE_COMPOSITING_MODE": "1",
                "WEBKIT_DISABLE_DMABUF_RENDERER": "1",
                # bwrap needs user namespaces the container doesn't have.
                "WEBKIT_DISABLE_SANDBOX_THIS_IS_DANGEROUS": "1",
                "GDK_BACKEND": "x11",
                "NO_AT_BRIDGE": "1",
                "LIBGL_ALWAYS_SOFTWARE": "1",
            }
        )
        logf = open(self.log_path, "ab")
        self.proc = subprocess.Popen(
            ["dbus-run-session", "--", self.appimage],
            stdout=logf,
            stderr=subprocess.STDOUT,
            env=env,
            start_new_session=True,
        )
        log(f"  Launched {os.path.basename(self.appimage)} (pid {self.proc.pid})")

    def terminate(self):
        if self.proc and self.proc.poll() is None:
            try:
                os.killpg(os.getpgid(self.proc.pid), signal.SIGTERM)
            except ProcessLookupError:
                pass
            try:
                self.proc.wait(timeout=15)
            except subprocess.TimeoutExpired:
                os.killpg(os.getpgid(self.proc.pid), signal.SIGKILL)
                self.proc.wait(timeout=10)
        # The sidecar backend is cleaned up by the shell on exit; make sure
        # stragglers from THIS instance don't hold the inspector port.
        subprocess.run(["pkill", "-f", "WebKitWebProcess"], check=False)
        time.sleep(2)


async def wait_for_ready(insp, timeout=120):
    """Wait until the page JS context has the Tauri IPC bridge."""
    deadline = time.time() + timeout
    while time.time() < deadline:
        try:
            r = await insp.eval_sync(
                "typeof window.__TAURI_INTERNALS__ !== 'undefined' ? 'ready' : 'no-bridge'"
            )
            if r == "ready":
                return
            dbg(f"bridge: {r}")
        except Exception as e:  # noqa: BLE001
            dbg(f"ready poll: {e!r}")
        await asyncio.sleep(2)
    raise TimeoutError("Tauri IPC bridge never appeared in the page")


# ── manifest juggling ────────────────────────────────────────────────────────


def write_manifest(dist_dir, base_manifest, signature=None, url=None, tag=""):
    manifest = json.loads(json.dumps(base_manifest))
    plat = manifest["platforms"]["linux-x86_64"]
    if signature is not None:
        plat["signature"] = signature
    if url is not None:
        plat["url"] = url
    path = os.path.join(dist_dir, "latest.json")
    with open(path, "w") as f:
        json.dump(manifest, f, indent=2)
    if tag:
        shutil.copy(path, os.path.join(dist_dir, f"latest-{tag}.snapshot.json"))
    return manifest


def expected_installed_sha(artifact):
    """The updater writes the raw AppImage: either the artifact itself (v2
    format) or the AppImage member of the legacy .tar.gz."""
    if artifact.endswith(".tar.gz"):
        with tarfile.open(artifact, "r:gz") as tf:
            for member in tf.getmembers():
                if member.name.endswith(".AppImage"):
                    h = hashlib.sha256()
                    f = tf.extractfile(member)
                    for chunk in iter(lambda: f.read(1 << 20), b""):
                        h.update(chunk)
                    return h.hexdigest()
        raise RuntimeError("no .AppImage member in the tar.gz updater artifact")
    return sha256(artifact)


# ── test phases ──────────────────────────────────────────────────────────────


async def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--appimage", required=True)
    ap.add_argument("--v99-artifact", required=True)
    ap.add_argument("--tampered-name", required=True)
    ap.add_argument("--wrong-sig", required=True)
    ap.add_argument("--dist-dir", required=True)
    ap.add_argument("--work-dir", required=True)
    ap.add_argument("--inspector-port", type=int, default=9222)
    ap.add_argument("--update-port", type=int, default=8090)
    ap.add_argument("--install", action="store_true")
    args = ap.parse_args()

    evidence_dir = os.path.join(args.work_dir, "evidence")
    os.makedirs(evidence_dir, exist_ok=True)
    evidence = {
        "started_at": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
        "appimage": args.appimage,
        "v99_artifact": args.v99_artifact,
        "results": {},
    }
    failures = []

    def record(test, ok, detail):
        evidence["results"][test] = {"pass": bool(ok), "detail": detail}
        status = "PASS" if ok else "FAIL"
        log(f"    {status}: {detail}")
        if not ok:
            failures.append(test)

    with open(os.path.join(args.work_dir, "latest-good.json")) as f:
        good_manifest = json.load(f)
    good_sig = good_manifest["platforms"]["linux-x86_64"]["signature"]
    good_url = good_manifest["platforms"]["linux-x86_64"]["url"]
    with open(args.wrong_sig) as f:
        wrong_sig = f.read().strip()

    base_sha = sha256(args.appimage)
    v99_sha = expected_installed_sha(args.v99_artifact)
    evidence["sha256"] = {"base_appimage_v0.0.1": base_sha, "v99_installed_expectation": v99_sha}
    log(f"  sha256(v0.0.1 AppImage)      = {base_sha}")
    log(f"  sha256(v99 install expected) = {v99_sha}")

    # Ensure the good manifest is what's being served for the check phase.
    write_manifest(args.dist_dir, good_manifest, tag="good")

    app = App(args.appimage, os.path.join(args.work_dir, "logs", "app-v0.0.1.log"), args.inspector_port)
    app.launch()
    try:
        insp = await Inspector.attach(args.inspector_port)
        await wait_for_ready(insp)

        log("\n  T1: get_app_version")
        version = await insp.eval_sync(
            "window.__TAURI_INTERNALS__.invoke ? 'has-invoke' : 'no-invoke'"
        )
        dbg(f"invoke probe: {version}")
        status, value = await insp.invoke_async(
            "window.__TAURI_INTERNALS__.invoke('get_app_version')", timeout=30
        )
        version = json.loads(value) if status == "ok" else value
        record("T1_get_app_version", status == "ok" and version == "0.0.1",
               f"get_app_version -> {version!r} (expected '0.0.1')")

        log("\n  T2: check_for_update detects 99.0.0")
        status, value = await insp.invoke_async(
            "window.__TAURI_INTERNALS__.invoke('check_for_update')", timeout=60
        )
        update_info = json.loads(value) if status == "ok" else None
        ok = (
            status == "ok"
            and update_info is not None
            and update_info.get("version") == "99.0.0"
            and update_info.get("currentVersion") == "0.0.1"
        )
        record("T2_check_for_update", ok,
               f"check_for_update -> {value if status == 'ok' else 'ERR: ' + str(value)}")
        evidence["check_for_update"] = update_info

        if not args.install:
            log("\n  T3-T6 skipped (run with --install for the full proof)")
        else:
            log("\n  T3: REJECT update signed by the wrong key (pubkey pinning)")
            write_manifest(args.dist_dir, good_manifest, signature=wrong_sig, tag="wrong-key")
            status, value = await insp.invoke_async(
                "window.__TAURI_INTERNALS__.invoke('download_and_install_update')", timeout=300
            )
            unchanged = sha256(args.appimage) == base_sha
            ok = status == "err" and unchanged
            record("T3_reject_wrong_key", ok,
                   f"status={status} (want err), error={str(value)[:200]!r}, appimage unchanged={unchanged}")

            log("\n  T4: REJECT tampered artifact bytes under the good signature")
            tampered_url = good_url.rsplit("/", 1)[0] + "/" + args.tampered_name
            write_manifest(args.dist_dir, good_manifest, signature=good_sig,
                           url=tampered_url, tag="tampered-bytes")
            status, value = await insp.invoke_async(
                "window.__TAURI_INTERNALS__.invoke('download_and_install_update')", timeout=300
            )
            unchanged = sha256(args.appimage) == base_sha
            ok = status == "err" and unchanged
            record("T4_reject_tampered_bytes", ok,
                   f"status={status} (want err), error={str(value)[:200]!r}, appimage unchanged={unchanged}")

            log("\n  T5: install the good signed update (AppImage replaced on disk)")
            write_manifest(args.dist_dir, good_manifest, tag="good")
            status, value = await insp.invoke_async(
                "window.__TAURI_INTERNALS__.invoke('download_and_install_update')", timeout=600,
            )
            installed_sha = sha256(args.appimage)
            ok = status == "ok" and installed_sha == v99_sha
            record("T5_install_good_update", ok,
                   f"status={status}, sha256(installed)={installed_sha} "
                   f"(expected {v99_sha}), replaced={installed_sha == v99_sha}")
            evidence["sha256"]["installed_appimage"] = installed_sha

        await insp.close()
    finally:
        app.terminate()

    if args.install and not failures:
        log("\n  T6: relaunch the replaced AppImage -> reports 99.0.0")
        app2 = App(args.appimage, os.path.join(args.work_dir, "logs", "app-v99.log"), args.inspector_port)
        app2.launch()
        try:
            insp = await Inspector.attach(args.inspector_port)
            await wait_for_ready(insp)
            status, value = await insp.invoke_async(
                "window.__TAURI_INTERNALS__.invoke('get_app_version')", timeout=30
            )
            version = json.loads(value) if status == "ok" else value
            record("T6_relaunched_version", status == "ok" and version == "99.0.0",
                   f"relaunched get_app_version -> {version!r} (expected '99.0.0')")
            await insp.close()
        finally:
            app2.terminate()

    evidence["finished_at"] = time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime())
    evidence["failures"] = failures
    with open(os.path.join(evidence_dir, "result.json"), "w") as f:
        json.dump(evidence, f, indent=2)
    # Keep the served manifests + app logs as raw evidence.
    for snap in os.listdir(args.dist_dir):
        if snap.startswith("latest-") and snap.endswith(".snapshot.json"):
            shutil.copy(os.path.join(args.dist_dir, snap), evidence_dir)

    log("")
    if failures:
        log(f"  RESULT: FAILED ({', '.join(failures)})")
        sys.exit(1)
    log("  RESULT: ALL TESTS PASSED")


if __name__ == "__main__":
    asyncio.run(main())

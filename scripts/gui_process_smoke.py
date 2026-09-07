"""Verify packaged webview startup, graceful shutdown, and idle replay.

This is real native renderer/host evidence, not an interaction or visual review.
"""

import argparse
from contextlib import closing
import hashlib
import json
import os
from pathlib import Path
import signal
import sqlite3
import subprocess
import sys
import tempfile
import time


def database_digest(path):
    with closing(sqlite3.connect(f"file:{path.as_posix()}?mode=ro", uri=True)) as connection:
        if connection.execute("PRAGMA integrity_check").fetchone() != ("ok",):
            raise RuntimeError("packaged database integrity check failed")
        return hashlib.sha256("\n".join(connection.iterdump()).encode()).hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--browser-runtime", type=Path)
    args = parser.parse_args()
    if sys.platform not in ("win32", "darwin", "linux"):
        parser.error("unsupported desktop platform")
    binary = args.binary.resolve(strict=True)
    with tempfile.TemporaryDirectory(prefix="autoharness-native-", ignore_cleanup_errors=True) as temporary:
        data = Path(temporary)
        environment = {key: value for key, value in os.environ.items()
                       if not key.upper().startswith(("AUTOHARNESS_", "GEMINI_", "OPENAI_", "CODEX_"))}
        environment.update(AUTOHARNESS_DATA_DIR=str(data), AUTOHARNESS_WORKSPACE=str(data),
                           WEBVIEW2_USER_DATA_FOLDER=str(data / "webview"),
                           XDG_DATA_HOME=str(data / "xdg-data"), XDG_CACHE_HOME=str(data / "xdg-cache"))
        if args.browser_runtime:
            environment["WEBVIEW2_BROWSER_EXECUTABLE_FOLDER"] = str(args.browser_runtime.resolve(strict=True))
        digests = []
        log = data / "autoharness.log"
        for launch in (1, 2):
            process = subprocess.Popen([str(binary)], cwd=data, env=environment, start_new_session=sys.platform != "win32",
                                       stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
            try:
                deadline = time.monotonic() + 45
                while time.monotonic() < deadline:
                    if process.poll() is not None:
                        raise RuntimeError(f"native application exited before renderer readiness: {process.returncode}")
                    if log.exists() and log.read_text(encoding="utf-8").count("gui_renderer_ready") == launch:
                        break
                    time.sleep(0.1)
                else:
                    raise RuntimeError("native renderer did not acknowledge its baseline")
                if sys.platform == "win32":
                    # Ask the exact child window to close through the OS, not IPC.
                    subprocess.run(["powershell", "-NoProfile", "-NonInteractive", "-Command",
                                    "$p = Get-Process -Id $env:AUTOHARNESS_SMOKE_PID; "
                                    "if (-not $p.CloseMainWindow()) { exit 1 }"],
                                   env={**environment, "AUTOHARNESS_SMOKE_PID": str(process.pid)}, check=True)
                else:
                    process.send_signal(signal.SIGINT)
                if process.wait(timeout=15) != 0:
                    raise RuntimeError("native shutdown failed")
                if log.read_text(encoding="utf-8").count("app_stopped") != launch:
                    raise RuntimeError("native runtime did not finish its shutdown joins")
                digests.append(database_digest(data / "autoharness.sqlite3"))
            finally:
                if process.poll() is None:
                    if sys.platform == "win32":
                        subprocess.run(["taskkill", "/PID", str(process.pid), "/T", "/F"],
                                       stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, check=False)
                    else:
                        os.killpg(process.pid, signal.SIGKILL)
                    process.wait(timeout=10)
        if digests[0] != digests[1]:
            raise RuntimeError("idle native replay changed durable data")
        args.output.mkdir(parents=True, exist_ok=True)
        report = {"schema_version": 1, "status": "passed", "platform": sys.platform,
                  "binary_sha256": hashlib.sha256(binary.read_bytes()).hexdigest(),
                  "native_baselines_acknowledged": 2, "clean_shutdowns": 2, "idle_replay_equivalent": True}
        (args.output / "process-smoke.json").write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")


if __name__ == "__main__":
    main()

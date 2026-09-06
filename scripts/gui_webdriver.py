"""Exercise an installed GUI through native WebDriver, with isolated synthetic data.

Requires EdgeDriver on Windows, or WebKitWebDriver on Linux.
No frontend fixture, test IPC, provider credential, or production data is used.
"""

import argparse
import base64
import hashlib
import json
import os
from pathlib import Path
import subprocess
import signal
import socket
import tempfile
import time
import sys
from urllib.error import HTTPError, URLError
from urllib.request import ProxyHandler, Request, build_opener

ELEMENT = "element-6066-11e4-a52e-4f735466cecf"
OPENER = build_opener(ProxyHandler({}))


class Driver:
    def __init__(self, port, application_environment=None, application_directory=None):
        self.url = f"http://127.0.0.1:{port}"
        self.session = ""
        self.application = None
        self.application_environment = application_environment
        self.application_directory = application_directory

    def request(self, method, path, data=None, session=True):
        prefix = f"/session/{self.session}" if session else ""
        request = Request(self.url + prefix + path,
                          data=json.dumps(data).encode() if data is not None else None,
                          method=method, headers={"Content-Type": "application/json"})
        with OPENER.open(request, timeout=120 if not session and path == "/session" else 45) as response:
            result = json.load(response).get("value")
        if isinstance(result, dict) and result.get("error"):
            raise RuntimeError("native WebDriver command failed")
        return result

    def start(self, binary):
        if sys.platform == "win32":
            with socket.socket() as reservation:
                reservation.bind(("127.0.0.1", 0))
                debugger_port = reservation.getsockname()[1]
            # An explicit test-only loopback connection avoids version-dependent
            # DevToolsActivePort discovery. Production launch has no debug port.
            environment = {**self.application_environment, "WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS":
                           f"--remote-debugging-port={debugger_port} --remote-debugging-address=127.0.0.1"}
            self.application = subprocess.Popen([str(binary)], cwd=self.application_directory, env=environment,
                                                stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
            self.wait(lambda: self.debugger_ready(debugger_port))
            capabilities = {"browserName": "webview2", "ms:edgeChromium": True,
                            "ms:edgeOptions": {"debuggerAddress": f"127.0.0.1:{debugger_port}"}}
        else:
            capabilities = {"webkitgtk:browserOptions": {"binary": str(binary), "args": []}}
        result = self.request("POST", "/session", {"capabilities": {"alwaysMatch": capabilities}}, session=False)
        self.session = result["sessionId"]
        self.wait(lambda: self.script("return Boolean(window.__TAURI_INTERNALS__) && "
                                     "Boolean(document.querySelector('button[aria-label=\"Sessions\"]'))"))

    @staticmethod
    def debugger_ready(port):
        with OPENER.open(f"http://127.0.0.1:{port}/json/version", timeout=1) as response:
            return bool(json.load(response).get("webSocketDebuggerUrl"))

    @staticmethod
    def wait(predicate, seconds=30):
        deadline = time.monotonic() + seconds
        while time.monotonic() < deadline:
            try:
                result = predicate()
                if result:
                    return result
            except (HTTPError, URLError, RuntimeError, ConnectionError, TimeoutError):
                pass
            time.sleep(0.15)
        raise RuntimeError("native GUI condition timed out")

    def script(self, script):
        return self.request("POST", "/execute/sync", {"script": script, "args": []})

    def element(self, xpath):
        return self.wait(lambda: self.request("POST", "/element", {"using": "xpath", "value": xpath}))[ELEMENT]

    def click(self, label):
        element = self.element(f"//button[@aria-label='{label}' or normalize-space(.)='{label}']")
        self.wait(lambda: self.request("GET", f"/element/{element}/enabled"))
        self.request("POST", f"/element/{element}/click", {})

    def fill(self, label, text):
        element = self.element(f"//input[@id=//label[normalize-space(.)='{label}']/@for]")
        self.wait(lambda: self.script("return !document.getAnimations().some(a => a.playState === 'running' && Number.isFinite(a.effect?.getComputedTiming().endTime))"))
        self.request("POST", f"/element/{element}/click", {})
        # Use the same keyboard events as a user so controlled React inputs see
        # selection and deletion consistently in WebKitGTK and WebView2.
        actions = [{"type": "keyDown", "value": "\ue009"}, {"type": "keyDown", "value": "a"},
                   {"type": "keyUp", "value": "a"}, {"type": "keyUp", "value": "\ue009"},
                   {"type": "keyDown", "value": "\ue003"}, {"type": "keyUp", "value": "\ue003"}]
        actions.extend(action for character in text for action in
                       ({"type": "keyDown", "value": character}, {"type": "keyUp", "value": character}))
        self.request("POST", "/actions", {"actions": [{"type": "key", "id": "keyboard", "actions": actions}]})
        self.wait(lambda: self.request("GET", f"/element/{element}/property/value") == text)

    def title(self, title):
        self.wait(lambda: self.script("return document.querySelector('.sessionDetailPane h2')?.textContent === " + json.dumps(title)))

    def painted(self):
        self.wait(lambda: self.script("return document.fonts.status === 'loaded' && !document.getAnimations().some(a => a.playState === 'running' && Number.isFinite(a.effect?.getComputedTiming().endTime))"))
        # GTK resize acknowledgement precedes compositor presentation. Two frame
        # callbacks plus a short presentation interval avoid stale backing pixels.
        self.request("POST", "/execute/async", {"script":
                     "const done = arguments[arguments.length - 1]; requestAnimationFrame(() => requestAnimationFrame(() => setTimeout(() => done(true), 300)));",
                     "args": []})

    def screenshot_matrix(self, output, route):
        for name, width, height in (("compact", 900, 640), ("standard", 1280, 800), ("wide", 1600, 1000)):
            self.request("POST", "/window/rect", {"width": width, "height": height})
            # WebDriver sets outer window bounds; evidence names exact CSS viewports.
            actual = self.script("return [innerWidth, innerHeight]")
            self.request("POST", "/window/rect", {"width": width + width - actual[0], "height": height + height - actual[1]})
            self.wait(lambda: self.script("return [innerWidth, innerHeight]") == [width, height])
            self.wait(lambda: self.script("return document.documentElement.scrollWidth <= innerWidth"))
            self.painted()
            (output / f"{route}-{name}.png").write_bytes(base64.b64decode(self.request("GET", "/screenshot")))
            if route == "sessions" and name == "compact":
                self.wait(lambda: self.script("return getComputedStyle(document.querySelector('.sessionDetailPane')).display !== 'none'"))
                self.click("Rename")
                self.element("//input[@id=//label[normalize-space(.)='New title']/@for]")
                self.painted()
                self.wait(lambda: self.script("const r = document.querySelector('[role=dialog]').getBoundingClientRect(); return r.left >= 0 && r.top >= 0 && r.right <= innerWidth && r.bottom <= innerHeight"))
                (output / "sessions-compact-rename.png").write_bytes(base64.b64decode(self.request("GET", "/screenshot")))
                self.click("Cancel")

    def close(self):
        try:
            if self.session:
                try:
                    self.request("DELETE", "", session=True)
                except HTTPError as error:
                    if error.code != 404:
                        raise
                finally:
                    self.session = ""
        finally:
            if self.application is not None:
                if self.application.poll() is None:
                    subprocess.run(["taskkill", "/PID", str(self.application.pid), "/T", "/F"],
                                   stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, check=False)
                    self.application.wait(timeout=10)
                self.application = None

    def clean_close(self, data):
        log = data / "autoharness.log"
        before = log.read_text(encoding="utf-8").count("app_stopped")
        # WebDriver's close-window endpoint destroys the webview instead of
        # delivering the native window-manager close request on Windows.
        self.request("POST", "/actions", {"actions": [{"type": "key", "id": "keyboard", "actions": [
            {"type": "keyDown", "value": "\ue009"}, {"type": "keyDown", "value": "k"},
            {"type": "keyUp", "value": "k"}, {"type": "keyUp", "value": "\ue009"}]}]})
        element = self.element("//button[@role='menuitem'][.//strong[text()='Quit AutoHarness']]")
        self.request("POST", f"/element/{element}/click", {})
        self.wait(lambda: log.read_text(encoding="utf-8").count("app_stopped") > before)
        self.close()


def journey(driver, binary, output, data):
    title = "Packaged lifecycle fixture"
    driver.start(binary)
    driver.screenshot_matrix(output, "onboarding")
    driver.click("Sessions")
    driver.click("Rename")
    driver.fill("New title", title)
    driver.click("Save title")
    driver.title(title)
    driver.screenshot_matrix(output, "sessions")
    driver.click("Archive")
    driver.click("Archive this session")
    driver.wait(lambda: driver.script("return !document.querySelector('[role=dialog]')"))
    # Archived sessions are intentionally absent from the default Open filter.
    archived = driver.element("//button[starts-with(normalize-space(.), 'Archived ')]")
    driver.request("POST", f"/element/{archived}/click", {})
    driver.title(title)
    driver.click("Restore session")
    driver.wait(lambda: driver.script("return document.querySelector('.sessionActionMessage')?.textContent.includes('Restored')"))
    # Deleting the only open session is deliberately forbidden by the runtime.
    driver.click("Create new session")
    driver.click("Sessions")
    driver.wait(lambda: driver.script("return document.querySelectorAll('.sessionWorkspaceRow').length === 2 && document.querySelector('.sessionWorkspaceRow[data-active=true] strong')?.textContent !== " + json.dumps(title)))
    driver.clean_close(data)
    driver.start(binary)
    driver.click("Sessions")
    row = driver.element("//button[contains(@class, 'sessionWorkspaceRow')][.//strong[text()='" + title + "']]")
    driver.request("POST", f"/element/{row}/click", {})
    driver.title(title)
    driver.click("Export Markdown")
    driver.wait(lambda: list(data.glob("*.md")))
    driver.click("Delete")
    driver.wait(lambda: driver.script("return Array.from(document.querySelectorAll('button')).some(b => b.textContent === 'Delete permanently' && b.disabled)"))
    driver.fill("Confirm session title", title)
    driver.click("Delete permanently")
    driver.wait(lambda: driver.script("return !document.querySelector('[role=dialog]') && !Array.from(document.querySelectorAll('.sessionWorkspaceRow strong')).some(e => e.textContent === " + json.dumps(title) + ")"))
    driver.close()
    driver.start(binary)
    driver.click("Sessions")
    driver.wait(lambda: driver.script("return !Array.from(document.querySelectorAll('.sessionWorkspaceRow strong')).some(e => e.textContent === " + json.dumps(title) + ")"))
    for route in ("Providers", "Memory", "Settings"):
        driver.click(route)
        driver.wait(lambda: driver.script("return document.querySelector('#main-content h1')?.textContent === " + json.dumps(route)))
        driver.screenshot_matrix(output, route.lower())
    driver.close()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--browser-runtime", type=Path, help="exact Windows WebView2 runtime directory matched to EdgeDriver")
    parser.add_argument("--native-driver", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--port", type=int, default=4444)
    args = parser.parse_args()
    if sys.platform not in ("win32", "linux"):
        parser.error("native WebDriver is supported only on Windows and Linux")
    binary = args.binary.resolve(strict=True)
    args.output.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="autoharness-gui-e2e-", ignore_cleanup_errors=True) as temporary:
        data = Path(temporary)
        environment = {key: value for key, value in os.environ.items()
                       if not key.upper().startswith(("AUTOHARNESS_", "GEMINI_", "OPENAI_", "CODEX_"))}
        environment.update(AUTOHARNESS_DATA_DIR=str(data), AUTOHARNESS_WORKSPACE=str(data),
                           WEBVIEW2_USER_DATA_FOLDER=str(data / "webview"),
                           TAURI_WEBVIEW_AUTOMATION="true",
                           XDG_DATA_HOME=str(data / "xdg-data"), XDG_CACHE_HOME=str(data / "xdg-cache"))
        # Never capture native driver logs: they can contain DOM and IPC payloads.
        if args.browser_runtime:
            environment["WEBVIEW2_BROWSER_EXECUTABLE_FOLDER"] = str(args.browser_runtime.resolve(strict=True))
        carrier = [str(args.native_driver.resolve(strict=True)), f"--port={args.port}"]
        process = subprocess.Popen(carrier,
                                   env=environment, cwd=data, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
                                   start_new_session=sys.platform != "win32")
        driver = Driver(args.port, environment, data)
        try:
            driver.wait(lambda: driver.request("GET", "/status", session=False))
            journey(driver, binary, args.output, data)
            report = {"schema_version": 1, "binary_sha256": hashlib.sha256(binary.read_bytes()).hexdigest(),
                      "journey": "offline-session-lifecycle", "status": "passed",
                      "graphics_mode": ("software-no-compositing" if sys.platform == "linux" and
                                        environment.get("WEBKIT_DISABLE_COMPOSITING_MODE") == "1" else "system-default"),
                      "restart_boundaries": 2, "clean_shutdown_verified": True, "visual_review": "pending"}
            (args.output / "lifecycle.json").write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")
        except Exception as failure:
            log = data / "autoharness.log"
            markers = log.read_text(encoding="utf-8") if log.exists() else ""
            diagnostics = {"schema_version": 1, "status": "failed", "failure_type": type(failure).__name__,
                           "carrier_exit_code": process.poll(), "session_created": bool(driver.session),
                           "app_started": markers.count('event="app_started"'),
                           "renderer_ready": markers.count('event="gui_renderer_ready"'),
                           "app_stopped": markers.count('event="app_stopped"')}
            if isinstance(failure, HTTPError):
                diagnostics["http_status"] = failure.code
                try:
                    message = json.load(failure).get("value", {}).get("message", "").lower()
                    diagnostics["driver_error_categories"] = [category for category in
                        ("devtoolsactiveport", "cannot find", "version", "disconnected", "not reachable",
                         "timed out", "user data", "pipe") if category in message]
                except (ValueError, AttributeError, OSError):
                    pass
            (args.output / "failure.json").write_text(json.dumps(diagnostics, indent=2) + "\n", encoding="utf-8")
            if driver.session:
                try:
                    (args.output / "failure.png").write_bytes(base64.b64decode(driver.request("GET", "/screenshot")))
                except (HTTPError, URLError, RuntimeError, ConnectionError, TimeoutError):
                    pass
            raise
        finally:
            try:
                driver.close()
            finally:
                if sys.platform == "win32":
                    subprocess.run(["taskkill", "/PID", str(process.pid), "/T", "/F"],
                                   stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, check=False)
                else:
                    os.killpg(process.pid, signal.SIGTERM)
                process.wait(timeout=10)


if __name__ == "__main__":
    main()

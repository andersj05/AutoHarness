from pathlib import Path
from contextlib import closing
import sqlite3
import tempfile
import unittest
from unittest.mock import Mock, patch
from urllib.error import HTTPError

from gui_process_smoke import database_digest
from gui_webdriver import Driver


class NativeToolTests(unittest.TestCase):
    def test_failed_driver_close_still_stops_its_owned_application(self):
        driver = Driver(4444)
        driver.session = "fixture"
        application = Mock(pid=4321)
        application.poll.return_value = None
        driver.application = application
        failure = HTTPError("http://127.0.0.1", 500, "failure", {}, None)
        self.addCleanup(failure.close)
        driver.request = Mock(side_effect=failure)
        with patch("gui_webdriver.subprocess.run") as terminate, self.assertRaises(HTTPError):
            driver.close()
        self.assertEqual(terminate.call_args.args[0], ["taskkill", "/PID", "4321", "/T", "/F"])
        application.wait.assert_called_once_with(timeout=10)
        self.assertIsNone(driver.application)
        self.assertEqual(driver.session, "")

    def test_edge_attachment_uses_an_explicit_loopback_test_child(self):
        environment = {"WEBVIEW2_USER_DATA_FOLDER": "C:/fixture/profile"}
        driver = Driver(4444, environment, Path("C:/fixture"))
        driver.request = Mock(side_effect=[{"sessionId": "fixture"}, True])
        with patch("gui_webdriver.sys.platform", "win32"), patch("gui_webdriver.subprocess.Popen") as launch, \
                patch.object(driver, "debugger_ready", return_value=True):
            driver.start(Path("C:/fixture/app.exe"))
        capabilities = driver.request.call_args_list[0].args[2]["capabilities"]["alwaysMatch"]
        address = capabilities["ms:edgeOptions"]["debuggerAddress"]
        self.assertTrue(address.startswith("127.0.0.1:"))
        child_environment = launch.call_args.kwargs["env"]
        self.assertEqual(child_environment["WEBVIEW2_USER_DATA_FOLDER"], environment["WEBVIEW2_USER_DATA_FOLDER"])
        self.assertIn(f"--remote-debugging-port={address.split(':')[1]}", child_environment["WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS"])
        self.assertNotIn("WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS", environment)

    def test_readiness_poll_tolerates_native_driver_startup_disconnect(self):
        readiness = Mock(side_effect=[ConnectionResetError(), True])
        self.assertTrue(Driver.wait(readiness, seconds=1))
        self.assertEqual(readiness.call_count, 2)

    def test_idle_replay_digest_is_stable_and_detects_durable_changes(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "fixture.sqlite3"
            with closing(sqlite3.connect(path)) as connection, connection:
                connection.execute("CREATE TABLE sessions (id TEXT PRIMARY KEY, title TEXT)")
                connection.execute("INSERT INTO sessions VALUES ('fixture', 'Before')")
            before = database_digest(path)
            self.assertEqual(database_digest(path), before)
            with closing(sqlite3.connect(path)) as connection, connection:
                connection.execute("UPDATE sessions SET title = 'After'")
            self.assertNotEqual(database_digest(path), before)


if __name__ == "__main__":
    unittest.main()

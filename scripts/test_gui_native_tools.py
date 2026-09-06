from pathlib import Path
from contextlib import closing
import sqlite3
import tempfile
import unittest
from unittest.mock import Mock

from gui_process_smoke import database_digest
from gui_webdriver import Driver


class NativeToolTests(unittest.TestCase):
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

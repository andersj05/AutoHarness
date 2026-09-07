import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import Mock, patch

import validate_gui


class LocalValidationTests(unittest.TestCase):
    def run_baseline(self, directory, codes):
        with patch("sys.argv", ["validate_gui.py", "--output", directory]), \
                patch.object(validate_gui, "commands", return_value=[["fixture", "first"], ["fixture", "second"]]), \
                patch.object(validate_gui.shutil, "which", return_value="fixture"), \
                patch.object(validate_gui.subprocess, "check_output", side_effect=["a" * 40, b" M fixture"]), \
                patch.object(validate_gui.subprocess, "run", side_effect=[Mock(returncode=code) for code in codes]) as run:
            if codes[0]:
                with self.assertRaises(SystemExit):
                    validate_gui.main()
            else:
                validate_gui.main()
            return run.call_count

    def test_failure_stops_before_later_checks_and_records_dirty_candidate(self):
        with tempfile.TemporaryDirectory() as directory:
            self.assertEqual(self.run_baseline(directory, [1]), 1)
            report = json.loads((Path(directory) / "baseline.json").read_text())
            self.assertEqual(report["status"], "failed")
            self.assertTrue(report["dirty"])
            self.assertEqual(report["checks"][0]["exit_code"], 1)
            self.assertFalse((Path(directory) / "02.log").exists())

    def test_success_requires_every_check(self):
        with tempfile.TemporaryDirectory() as directory:
            self.assertEqual(self.run_baseline(directory, [0, 0]), 2)
            report = json.loads((Path(directory) / "baseline.json").read_text())
            self.assertEqual(report["status"], "passed")
            self.assertEqual(len(report["checks"]), 2)


if __name__ == "__main__":
    unittest.main()

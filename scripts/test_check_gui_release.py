import copy
from datetime import date
import hashlib
import json
from pathlib import Path
import tempfile
import unittest

from check_gui_release import APPROVALS, COMMON_GATES, PLATFORM_GATES, PLATFORMS, check, template


class ReleaseGateTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.commit = "a" * 40
        proof = self.write("proof.txt", "Synthetic evidence for validator tests")
        passed = {"status": "passed", "commit": self.commit, "evidence": proof}
        self.record = {"schema_version": 1, "commit": self.commit, "open_p0_p1": [], "blockers": [],
                       "gates": {name: copy.deepcopy(passed) for name in COMMON_GATES}, "platforms": {},
                       "approvals": {name: copy.deepcopy(passed) for name in (*APPROVALS, "tui-retirement")},
                       "rollback": {"previous_commit": "b" * 40, "window_closes": "2026-09-20", "rehearsal": proof}}
        for platform in PLATFORMS:
            artifact = self.write(f"{platform}/fixture.bin", "synthetic artifact")
            manifest = {"schema_version": 1, "commit": self.commit, "platform": platform,
                        "profile": "release", "signature_verified": True,
                        "artifacts": [{"name": "fixture.bin", "sha256": artifact["sha256"]}]}
            self.record["platforms"][platform] = {
                "package": self.write(f"{platform}/manifest.json", json.dumps(manifest)),
                "gates": {name: copy.deepcopy(passed) for name in PLATFORM_GATES}}

    def write(self, name, content):
        path = self.root / name
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(content, encoding="utf-8")
        return {"path": name, "sha256": hashlib.sha256(path.read_bytes()).hexdigest()}

    def test_complete_candidate_and_closed_window(self):
        check(self.record, self.root, self.commit)
        check(self.record, self.root, self.commit, retire=True, today=date(2026, 9, 21))

    def test_template_is_not_release_evidence(self):
        with self.assertRaises(ValueError):
            check(template(), self.root, self.commit)

    def test_every_gate_and_approval_is_required_for_same_commit(self):
        groups = [self.record["gates"], self.record["approvals"]]
        groups.extend(self.record["platforms"][p]["gates"] for p in PLATFORMS)
        for group in groups:
            for name in group:
                if name == "tui-retirement":
                    continue
                with self.subTest(gate=name):
                    group[name]["commit"] = "c" * 40
                    with self.assertRaises(ValueError):
                        check(self.record, self.root, self.commit)
                    group[name]["commit"] = self.commit

    def test_changed_artifact_is_rejected(self):
        (self.root / "win32/fixture.bin").write_text("tampered", encoding="utf-8")
        with self.assertRaisesRegex(ValueError, "SHA-256"):
            check(self.record, self.root, self.commit)

    def test_unsigned_debug_and_wrong_candidate_packages_are_rejected(self):
        for field, value in (("signature_verified", False), ("profile", "debug"), ("commit", "c" * 40)):
            with self.subTest(field=field):
                path = self.root / "linux/manifest.json"
                original = path.read_text(encoding="utf-8")
                manifest = json.loads(original)
                manifest[field] = value
                self.record["platforms"]["linux"]["package"] = self.write("linux/manifest.json", json.dumps(manifest))
                with self.assertRaisesRegex(ValueError, "unsigned, debug"):
                    check(self.record, self.root, self.commit)
                self.record["platforms"]["linux"]["package"] = self.write("linux/manifest.json", original)

    def test_retirement_requires_expired_window_and_separate_approval(self):
        with self.assertRaisesRegex(ValueError, "window"):
            check(self.record, self.root, self.commit, retire=True, today=date(2026, 9, 20))
        self.record["approvals"].pop("tui-retirement")
        with self.assertRaises(ValueError):
            check(self.record, self.root, self.commit, retire=True, today=date(2026, 9, 21))

    def test_path_escape_and_open_defects_are_rejected(self):
        self.record["gates"]["security-review"]["evidence"]["path"] = str(self.root / "proof.txt")
        with self.assertRaises(ValueError):
            check(self.record, self.root, self.commit)
        self.record["open_p0_p1"] = ["P1 fixture"]
        with self.assertRaisesRegex(ValueError, "unresolved"):
            check(self.record, self.root, self.commit)


if __name__ == "__main__":
    unittest.main()

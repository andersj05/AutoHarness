"""Fail closed unless one GUI candidate has all release evidence and approvals.

This checks evidence completeness and byte identity, not the truth of human
attestations. It never publishes or changes source defaults.
"""

import argparse
from datetime import date
import hashlib
import json
from pathlib import Path
import re
import sys

PLATFORMS = ("win32", "darwin", "linux")
COMMON_GATES = ("rust-baseline", "frontend-baseline", "documentation", "renderer-independence",
                "help-parity", "security-review", "performance", "migration-rollback")
PLATFORM_GATES = ("installer-lifecycle", "native-session-lifecycle", "live-providers", "gui-vault",
                  "permission-recovery", "memory-replay", "crash-recovery", "keyboard-screen-reader",
                  "native-visual-compact", "native-visual-standard", "native-visual-wide")
APPROVALS = ("release", "rollback", "default-interface")


def evidence_file(root, record):
    if not isinstance(record, dict) or set(record) != {"path", "sha256"}:
        raise ValueError("evidence requires exactly path and sha256")
    relative = Path(record["path"])
    if relative.is_absolute():
        raise ValueError("evidence paths must be relative")
    path = (root / relative).resolve(strict=True)
    if not path.is_relative_to(root.resolve()) or not path.is_file() or path.stat().st_size == 0:
        raise ValueError("evidence must be a nonempty file inside the evidence directory")
    with path.open("rb") as source:
        digest = hashlib.file_digest(source, "sha256").hexdigest()
    if digest != record["sha256"]:
        raise ValueError("evidence SHA-256 mismatch")
    return path


def attestation(root, record, commit):
    if not isinstance(record, dict) or record.get("status") != "passed" or record.get("commit") != commit:
        raise ValueError("gate must pass for the exact candidate commit")
    evidence_file(root, record.get("evidence"))


def check(record, root, commit):
    if not re.fullmatch(r"[0-9a-f]{40}", commit):
        raise ValueError("candidate must be a full lowercase Git commit")
    if record.get("schema_version") != 1 or record.get("commit") != commit:
        raise ValueError("release record does not match this candidate")
    if record.get("open_p0_p1") != [] or record.get("blockers") != []:
        raise ValueError("unresolved defects or release blockers")
    for gate in COMMON_GATES:
        attestation(root, record.get("gates", {}).get(gate), commit)
    for platform in PLATFORMS:
        platform_record = record.get("platforms", {}).get(platform, {})
        manifest_path = evidence_file(root, platform_record.get("package"))
        manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
        if (manifest.get("schema_version") != 1 or manifest.get("commit") != commit
                or manifest.get("platform") != platform or manifest.get("profile") != "release"
                or manifest.get("signature_verified") is not True):
            raise ValueError("package is unsigned, debug, or belongs to another candidate/platform")
        artifacts = manifest.get("artifacts", [])
        if not artifacts:
            raise ValueError("package has no artifacts")
        names = set()
        for artifact in artifacts:
            name = artifact.get("name", "")
            if not name or Path(name).name != name or name in names:
                raise ValueError("invalid or duplicate package artifact")
            names.add(name)
            evidence_file(manifest_path.parent, {"path": name, "sha256": artifact.get("sha256")})
        for gate in PLATFORM_GATES:
            attestation(root, platform_record.get("gates", {}).get(gate), commit)
    for approval in APPROVALS:
        attestation(root, record.get("approvals", {}).get(approval), commit)
    rollback = record.get("rollback", {})
    if not re.fullmatch(r"[0-9a-f]{40}", rollback.get("previous_commit", "")) or rollback.get("previous_commit") == commit:
        raise ValueError("rollback requires a distinct previous candidate")
    evidence_file(root, rollback.get("rehearsal"))
    date.fromisoformat(rollback.get("window_closes", ""))


def template():
    pending = {"status": "pending", "commit": "", "evidence": {"path": "", "sha256": ""}}
    return {"schema_version": 1, "commit": "", "open_p0_p1": [], "blockers": ["Release evidence pending"],
            "gates": {name: pending for name in COMMON_GATES},
            "platforms": {platform: {"package": {"path": "", "sha256": ""},
                           "gates": {name: pending for name in PLATFORM_GATES}} for platform in PLATFORMS},
            "approvals": {name: pending for name in APPROVALS},
            "rollback": {"previous_commit": "", "window_closes": "", "rehearsal": {"path": "", "sha256": ""}}}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("record", type=Path, nargs="?")
    parser.add_argument("--commit")
    parser.add_argument("--template", action="store_true")
    args = parser.parse_args()
    if args.template:
        print(json.dumps(template(), indent=2))
        return
    if not args.record or not args.commit:
        parser.error("record and --commit are required")
    try:
        check(json.loads(args.record.read_text(encoding="utf-8")), args.record.parent,
              args.commit)
    except (ValueError, OSError, TypeError, KeyError) as error:
        print(f"GUI promotion blocked: {error}", file=sys.stderr)
        sys.exit(1)
    print("GUI evidence is complete for the candidate; maintainer approval remains authoritative.")


if __name__ == "__main__":
    main()

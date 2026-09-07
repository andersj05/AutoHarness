"""Run the complete local GUI baseline; native signed release journeys are separate."""

import argparse
import json
from pathlib import Path
import shutil
import subprocess
import sys
import time

ROOT = Path(__file__).resolve().parents[1]


def commands():
    return [
        ["pnpm", "install", "--frozen-lockfile"],
        ["pnpm", "gui:typecheck"],
        ["pnpm", "gui:test"],
        ["pnpm", "gui:build"],
        [sys.executable, "-m", "unittest", "discover", "-s", "scripts", "-p", "test_*.py"],
        [sys.executable, "scripts/check_docs_links.py"],
        ["cargo", "fmt", "--all", "--", "--check"],
        ["pnpm", "gui:themes:check"],
        ["cargo", "clippy", "--workspace", "--all-targets", "--all-features", "--locked", "--no-deps", "--", "-D", "warnings"],
        ["cargo", "test", "--workspace", "--all-targets", "--all-features", "--locked", "--no-fail-fast"],
        ["cargo", "doc", "--workspace", "--all-features", "--no-deps", "--locked"],
        ["cargo", "test", "--workspace", "--doc", "--all-features", "--locked"],
        ["cargo", "fmt", "--manifest-path", "benchmarks/Cargo.toml", "--", "--check"],
        ["cargo", "clippy", "--manifest-path", "benchmarks/Cargo.toml", "--bin", "autoharness-phase1-benchmarks", "--locked", "--no-deps", "--", "-D", "warnings"],
        ["cargo", "test", "--manifest-path", "benchmarks/Cargo.toml", "--bin", "autoharness-phase1-benchmarks", "--locked"],
    ]


def main():
    import os

    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, default=ROOT / "target/gui-evidence/local-baseline")
    args = parser.parse_args()
    args.output.mkdir(parents=True, exist_ok=True)
    commit = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=ROOT, text=True).strip()
    dirty = bool(subprocess.check_output(["git", "status", "--porcelain"], cwd=ROOT))
    report = {"schema_version": 1, "commit": commit, "dirty": dirty, "status": "running", "checks": []}
    environment = {**os.environ, "RUSTDOCFLAGS": "-D warnings"}
    for index, command in enumerate(commands()):
        print(f"[{index + 1}] {' '.join(command)}", flush=True)
        started = time.monotonic()
        with (args.output / f"{index + 1:02d}.log").open("w", encoding="utf-8") as log:
            result = subprocess.run([shutil.which(command[0]) or command[0], *command[1:]],
                                    cwd=ROOT, env=environment, stdout=log, stderr=subprocess.STDOUT)
        report["checks"].append({"command": command, "exit_code": result.returncode,
                                 "seconds": round(time.monotonic() - started, 2)})
        report["status"] = "failed" if result.returncode else "running"
        (args.output / "baseline.json").write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")
        if result.returncode:
            raise SystemExit(f"Check failed; inspect {args.output / f'{index + 1:02d}.log'}")
    report["status"] = "passed"
    (args.output / "baseline.json").write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")
    print(f"Local baseline passed: {args.output / 'baseline.json'}", flush=True)


if __name__ == "__main__":
    main()

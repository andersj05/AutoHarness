"""Select CI jobs conservatively from the complete pull-request diff."""

import json
import os
from pathlib import Path
import subprocess


def classify(paths):
    scope = dict.fromkeys(("rust", "frontend", "benchmarks"), False)
    for path in paths:
        if path.endswith(".md") or path.startswith("docs/"):
            continue
        if path.startswith("scripts/"):
            continue  # Python tests and documentation checks always run.
        if path.startswith("apps/gui/") or path in (
            "package.json", "pnpm-lock.yaml", "pnpm-workspace.yaml", ".node-version", ".npmrc"
        ):
            scope["frontend"] = True
        elif path.startswith("benchmarks/"):
            scope["benchmarks"] = True
        elif path.startswith("crates/"):
            scope["rust"] = True
            scope["benchmarks"] = True  # Benchmarks consume workspace crates.
            if path.startswith(("crates/autoharness-client/", "crates/autoharness-presentation/")):
                scope["frontend"] = True
        else:
            return dict.fromkeys(scope, True)  # New infrastructure must not silently skip tests.
    return scope


def main():
    if os.environ.get("GITHUB_EVENT_NAME") == "pull_request":
        event = json.loads(Path(os.environ["GITHUB_EVENT_PATH"]).read_text(encoding="utf-8"))
        base = event["pull_request"]["base"]["sha"]
        # Compare the tested merge tree with its exact base, including deletes and renames.
        result = subprocess.check_output(["git", "diff", "--name-only", "--no-renames", "-z", base, "HEAD", "--"])
        scope = classify(result.decode("utf-8").split("\0")[:-1])
    else:
        scope = dict.fromkeys(("rust", "frontend", "benchmarks"), True)
    with open(os.environ["GITHUB_OUTPUT"], "a", encoding="utf-8") as output:
        for job, enabled in scope.items():
            output.write(f"{job}={str(enabled).lower()}\n")
    print(json.dumps(scope, sort_keys=True))


if __name__ == "__main__":
    main()

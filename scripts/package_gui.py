"""Build candidate installers without publishing or changing the source default.

Signed mode fails before building when signing configuration is missing.
Unsigned packages are explicitly development evidence, never release evidence.
"""

import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys
import time

ROOT = Path(__file__).resolve().parents[1]
APP = ROOT / "crates/autoharness-app"
FORMATS = {"win32": ("nsis", ".exe"), "darwin": ("dmg", ".dmg"), "linux": ("deb", ".deb")}


def signing_config(platform, environment):
    """Return a narrow Tauri override; never include credential values."""
    if platform == "win32":
        thumbprint = environment.get("AUTOHARNESS_WINDOWS_CERTIFICATE_THUMBPRINT", "")
        timestamp = environment.get("AUTOHARNESS_WINDOWS_TIMESTAMP_URL", "")
        if not re.fullmatch(r"[0-9a-fA-F]{40}", thumbprint) or not timestamp.startswith("https://"):
            raise ValueError("Windows signing requires a certificate thumbprint and HTTPS timestamp URL")
        return {"bundle": {"windows": {"certificateThumbprint": thumbprint,
                "digestAlgorithm": "sha256", "timestampUrl": timestamp, "tsp": True}}}
    if platform == "darwin":
        required = ("APPLE_SIGNING_IDENTITY", "APPLE_ID", "APPLE_PASSWORD", "APPLE_TEAM_ID")
        if not all(environment.get(key) for key in required):
            raise ValueError("macOS signing requires Developer ID identity and Apple notarization credentials")
        if not environment["APPLE_SIGNING_IDENTITY"].startswith("Developer ID Application:"):
            raise ValueError("macOS releases require a Developer ID Application identity")
        return {"bundle": {"macOS": {"signingIdentity": environment["APPLE_SIGNING_IDENTITY"]}}}
    if not environment.get("AUTOHARNESS_LINUX_SIGNING_KEY"):
        raise ValueError("Linux signing requires AUTOHARNESS_LINUX_SIGNING_KEY in an available GPG keyring")
    return {}


def run(arguments, **kwargs):
    return subprocess.run(arguments, check=True, **kwargs)


def candidate_identity():
    commit = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=ROOT, text=True).strip()
    if subprocess.check_output(["git", "status", "--porcelain", "--untracked-files=normal"], cwd=ROOT):
        raise ValueError("commit or stash working changes before building a candidate")
    return commit


def verify_signature(path, platform, environment):
    if platform == "win32":
        # Pass the path through the environment, never shell interpolation.
        run(["powershell", "-NoProfile", "-NonInteractive", "-Command",
             "$s = Get-AuthenticodeSignature -LiteralPath $env:AUTOHARNESS_VERIFY_ARTIFACT; "
             "if ($s.Status -ne 'Valid' -or "
             "$s.SignerCertificate.Thumbprint -ne $env:AUTOHARNESS_WINDOWS_CERTIFICATE_THUMBPRINT "
             "-or $null -eq $s.TimeStamperCertificate) { exit 1 }"],
            env={**environment, "AUTOHARNESS_VERIFY_ARTIFACT": str(path)})
    elif platform == "darwin":
        run(["xcrun", "stapler", "validate", str(path)])
        run(["spctl", "--assess", "--type", "open", "--context", "context:primary-signature", str(path)])
    else:
        run(["gpg", "--batch", "--yes", "--local-user", environment["AUTOHARNESS_LINUX_SIGNING_KEY"],
             "--armor", "--detach-sign", str(path)])
        run(["gpg", "--batch", "--verify", str(path) + ".asc", str(path)])


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--unsigned", action="store_true", help="development packages only")
    parser.add_argument("--debug", action="store_true", help="faster local lifecycle validation; never release evidence")
    args = parser.parse_args()
    if args.debug and not args.unsigned:
        parser.error("--debug requires --unsigned")
    platform = sys.platform
    if platform not in FORMATS:
        parser.error("unsupported desktop platform")
    environment = dict(os.environ)
    config = {} if args.unsigned else signing_config(platform, environment)
    commit = candidate_identity()
    bundle, extension = FORMATS[platform]
    # Reuse dependency compilation, but accept only artifacts written by this build.
    target = ROOT / "target"
    environment["CARGO_TARGET_DIR"] = str(target)
    started = time.time()
    command = [shutil.which("node") or "node", str(ROOT / "node_modules/@tauri-apps/cli/tauri.js"),
               "build", "--ci", "--config", str(APP / "tauri.package.conf.json"),
               "--features", "gui-package", "--bundles", bundle]
    if args.unsigned:
        command.append("--no-sign")
    else:
        command.extend(["--config", json.dumps(config)])
    if args.debug:
        command.append("--debug")
    command.extend(["--", "--locked"])
    run(command, cwd=APP, env=environment)
    profile = "debug" if args.debug else "release"
    artifacts = sorted((target / profile / "bundle" / bundle).glob("*" + extension))
    artifacts = [path for path in artifacts if path.stat().st_mtime >= started - 1]
    if len(artifacts) != 1:
        raise ValueError("expected exactly one newly built installer")
    artifact = artifacts[0]
    if not args.unsigned:
        verify_signature(artifact, platform, environment)
    if candidate_identity() != commit:
        raise ValueError("candidate changed during the build; no manifest will be issued")
    output = target / "gui-packages" / commit / platform / profile
    output.mkdir(parents=True, exist_ok=True)
    files = [artifact]
    if platform == "linux" and not args.unsigned:
        files.append(Path(str(artifact) + ".asc"))
    manifest = {"schema_version": 1, "commit": commit, "platform": platform,
                "profile": profile, "signature_verified": not args.unsigned,
                "rust_toolchain": subprocess.check_output(["rustc", "--version"], text=True).strip(),
                "node_version": subprocess.check_output(["node", "--version"], text=True).strip(),
                "artifacts": []}
    executable = target / profile / ("autoharness.exe" if platform == "win32" else "autoharness")
    with executable.open("rb") as source:
        manifest["binary_sha256"] = hashlib.file_digest(source, "sha256").hexdigest()
    for source in files:
        destination = output / source.name
        shutil.copy2(source, destination)
        with destination.open("rb") as content:
            manifest["artifacts"].append({"name": source.name,
                "sha256": hashlib.file_digest(content, "sha256").hexdigest()})
    (output / "manifest.json").write_text(json.dumps(manifest, indent=2) + "\n", encoding="utf-8")
    print(f"Candidate packages: {output.relative_to(ROOT)}")


if __name__ == "__main__":
    try:
        main()
    except (ValueError, subprocess.CalledProcessError) as error:
        # Do not echo child arguments because signing configuration can be sensitive.
        print(str(error) if isinstance(error, ValueError) else "Packaging command failed", file=sys.stderr)
        sys.exit(1)

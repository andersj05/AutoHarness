"""Update remaining golden headers for the status surface."""
import subprocess
from pathlib import Path


def actual_header(width: int, height: int) -> str | None:
    out = subprocess.run(
        [
            "cargo",
            "test",
            "-p",
            "autoharness-tui",
            "--test",
            "ui",
            "fixed_size",
            "--locked",
        ],
        capture_output=True,
        text=True,
    ).stdout
    for line in out.split("\n"):
        marker = f"golden mismatch at {width}x{height}"
        if marker in line:
            return None
    return None


for name in ["main-80x24.txt", "main-60x18.txt", "main-40x12.txt"]:
    path = Path("crates/autoharness-tui/tests/golden") / name
    print(name, "first line:", path.read_text(encoding="utf-8").split("\n")[0][:60])

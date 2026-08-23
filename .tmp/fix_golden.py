"""Update golden headers for the status surface (80x24, 60x18, 40x12)."""
from pathlib import Path

updates = {
    "main-80x24.txt": " AutoHarness  |  gemini (default)  |  session only  |  Gemini 2.5 Pro  |  failed",
    "main-60x18.txt": " AutoHarness  |  Gemini 2.5 Pro  |  failed",
    "main-40x12.txt": None,
}

for name, header in updates.items():
    if header is None:
        continue
    path = Path("crates/autoharness-tui/tests/golden") / name
    lines = path.read_text(encoding="utf-8").split("\n")
    lines[0] = header
    path.write_text("\n".join(lines), encoding="utf-8", newline="\n")
    print(name, "->", repr(lines[0][:70]))

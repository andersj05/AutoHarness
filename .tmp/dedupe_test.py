"""Remove the duplicated broken export test (first copy, lines 3232..3324)."""
from pathlib import Path

path = Path("crates/autoharness-app/src/coordinator.rs")
lines = path.read_text(encoding="utf-8").split("\n")

start = 3232 - 1
end = 3324
segment = "\n".join(lines[start:end])
assert "async fn slash_export_writes_markdown" in segment, "wrong slice"
del lines[start:end]
path.write_text("\n".join(lines), encoding="utf-8", newline="\n")
print(
    "removed",
    end - start,
    "lines; remaining copies:",
    sum(1 for l in lines if "slash_export_writes" in l),
)

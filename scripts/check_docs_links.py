#!/usr/bin/env python3
"""Check relative Markdown links and repository-memory consistency.

The documentation system routes readers through many cross-file links.
This script verifies that every relative link in every Markdown file
resolves to a real file and, when present, to a real heading anchor.
It also verifies that every architecture decision record in ``docs/adr``
is listed in the ADR index so records cannot be added silently.

Run from the repository root:

    python scripts/check_docs_links.py

Exit code 0 means every checked link resolves.
"""

from __future__ import annotations

import re
import sys
import unicodedata
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent
SKIPPED_DIRECTORIES = {".git", "target", "node_modules"}
INLINE_LINK = re.compile(r"\[[^\]]*\]\(([^)\s]+)\)")
REFERENCE_DEFINITION = re.compile(r"^\s*%s:\s*(<[^>]+>|[^)\s]+)\s*$" % re.escape("]"))
FENCED_CODE_BLOCK = re.compile(r"```.*?```", re.DOTALL)
EXTERNAL_SCHEME = re.compile(r"^[a-z][a-z0-9+.-]*:", re.IGNORECASE)
HEADING = re.compile(r"^(#{1,6})\s+(.*?)\s*$")


def markdown_files() -> list[Path]:
    files: list[Path] = []
    for path in sorted(REPO_ROOT.rglob("*.md")):
        if any(part in SKIPPED_DIRECTORIES for part in path.parts):
            continue
        files.append(path)
    return files


def github_slug(heading_text: str) -> str:
    normalized = unicodedata.normalize("NFKD", heading_text)
    stripped = "".join(
        character
        for character in normalized
        if character.isalnum() or character in {" ", "-", "_"}
    )
    return stripped.strip().lower().replace(" ", "-")


def headings_in(path: Path) -> set[str]:
    slugs: set[str] = set()
    for line in path.read_text(encoding="utf-8").splitlines():
        match = HEADING.match(line)
        if match:
            slugs.add(github_slug(match.group(2)))
    return slugs


def strip_code_spans(text: str) -> str:
    return re.sub(r"`[^`]*`", "", text)


def check_links(path: Path, errors: list[str]) -> None:
    raw_lines = path.read_text(encoding="utf-8").splitlines()
    text_without_fences = FENCED_CODE_BLOCK.sub("", "\n".join(raw_lines))
    lines_after_fences = text_without_fences.split("\n")
    headings = headings_in(path)

    for line_number, line in enumerate(lines_after_fences, start=1):
        cleaned = strip_code_spans(line)
        targets = INLINE_LINK.findall(cleaned)
        definition = REFERENCE_DEFINITION.match(cleaned)
        if definition:
            targets.append(definition.group(1).strip("<>"))

        for target in targets:
            destination, _, fragment = target.partition("#")
            if not destination:
                continue  # same-document anchor; heading checks below cover fragments
            if EXTERNAL_SCHEME.match(destination):
                continue
            resolved = (path.parent / Path(destination)).resolve()
            if not resolved.is_file():
                errors.append(
                    f"{path.relative_to(REPO_ROOT)}:{line_number}: "
                    f"broken link target '{destination}'"
                )
                continue
            if fragment and resolved.suffix == ".md":
                fragment_headings = (
                    headings if resolved == path else headings_in(resolved)
                )
                if github_slug(fragment) not in fragment_headings:
                    errors.append(
                        f"{path.relative_to(REPO_ROOT)}:{line_number}: "
                        f"missing anchor '#{fragment}' in '{destination}'"
                    )


def check_adr_index(errors: list[str]) -> None:
    adr_directory = REPO_ROOT / "docs" / "adr"
    index_path = adr_directory / "README.md"
    indexed = index_path.read_text(encoding="utf-8")
    template_prefix = "0000"
    for record in sorted(adr_directory.glob("*.md")):
        if record.name.startswith(template_prefix) or record.name == "README.md":
            continue
        if record.name not in indexed:
            errors.append(
                f"{record.relative_to(REPO_ROOT)}: not listed in docs/adr/README.md"
            )


def main() -> int:
    errors: list[str] = []
    files = markdown_files()
    for path in files:
        check_links(path, errors)
    check_adr_index(errors)

    if errors:
        print(f"{len(errors)} documentation problem(s) found:")
        for error in errors:
            print(f"  - {error}")
        return 1

    print(f"Checked {len(files)} Markdown files: all relative links resolve.")
    return 0


if __name__ == "__main__":
    sys.exit(main())

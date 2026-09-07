import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, expect, it } from "vitest";
import { InspectorSlot, type WorkspaceSurface } from "./slots";

afterEach(cleanup);

it("renders all six typed surfaces as inert text without links, styles, scripts, or commands", () => {
  const hostile = '<script>alert(1)</script><img src="https://invalid.test/x" onerror="alert(2)"><style>body{display:none}</style> javascript:alert(3)';
  const surfaces: WorkspaceSurface[] = [
    { kind: "plan", title: "Plan", steps: [{ label: hostile, state: "pending" }] },
    { kind: "artifact", title: "Artifact", identity: "artifact-1", mediaType: "text/html", content: hostile },
    { kind: "file", title: "File", path: "javascript:alert(4)", content: hostile },
    { kind: "diff", title: "Diff", before: hostile, after: hostile + "\nchanged" },
    { kind: "terminal_output", title: "Terminal output", command: "rm -rf /", output: "\u001b[31m" + hostile, exitCode: 1 },
    { kind: "evaluation", title: "Evaluation", status: "unreviewed", metrics: [{ label: hostile, value: "1" }] },
  ];
  const { container } = render(<InspectorSlot surfaces={surfaces} />);
  expect(container.querySelectorAll("script, img, style, iframe, a, button")).toHaveLength(0);
  expect(container.querySelectorAll("details")).toHaveLength(6);
  expect(container.textContent).toContain(hostile);
  expect(container.textContent).toContain("\\u{1b}[31m");
  expect(screen.getByLabelText("Proposed content comparison")).toHaveTextContent("Current content");
});

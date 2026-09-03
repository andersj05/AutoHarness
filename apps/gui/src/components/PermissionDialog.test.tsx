import { render, screen } from "@testing-library/react";
import { expect, it } from "vitest";

import type { PermissionRequest } from "../protocol";
import { securityDisplaySafe } from "../securityText";
import { PermissionDialog } from "./PermissionDialog";

it("exposes directional and invisible formatting controls", () => {
  const permission: PermissionRequest = {
    id: "tool-bidi",
    sessionId: "session-1",
    toolName: "workspace_read_v1",
    capability: "Filesystem read",
    resource: securityDisplaySafe("workspace:report.p\u{200b}df"),
    reason: "Review the exact operation.",
    trustedFields: [{ label: "Path", value: securityDisplaySafe("safe\u{202e}txt.exe") }],
  };

  const view = render(
    <PermissionDialog permission={permission} onAllow={() => undefined} onDeny={() => undefined} />,
  );

  expect(screen.getByText("safe\\u{202e}txt.exe")).toBeInTheDocument();
  expect(screen.getByText("workspace:report.p\\u{200b}df")).toBeInTheDocument();
  expect(view.container.textContent).not.toContain("\u{202e}");
  expect(view.container.textContent).not.toContain("\u{200b}");
  expect(securityDisplaySafe("safe\u{202e}txt.exe")).not.toBe(
    securityDisplaySafe("safe\\u{202e}txt.exe"),
  );
});

import type { ActiveSessionProjection } from "../../protocol";
import type { WorkspaceSurface } from "./slots";

/** Expose only bounded tool evidence already present in the authoritative transcript. */
export function toolEvidenceSurfaces(session?: ActiveSessionProjection): readonly WorkspaceSurface[] {
  return session?.transcript.filter((item) => item.kind === "tool").slice(-8).map((item) => ({
    kind: "artifact" as const, title: item.name, identity: item.id, mediaType: "text/plain",
    content: [item.resource, item.summary, item.detail, item.failure?.message].filter(Boolean).join("\n"),
  })) ?? [];
}

/** Fixtures demonstrate presentation contracts for runtimes that do not yet emit these surfaces. */
export const fixtureWorkspaceSurfaces: readonly WorkspaceSurface[] = [
  { kind: "plan", title: "Implementation plan", steps: [{ label: "Inspect authoritative memory", state: "complete" }, { label: "Review proposed changes", state: "active" }] },
  { kind: "artifact", title: "Retained evidence", identity: "fixture-artifact-1", mediaType: "text/plain", content: "A bounded, inert evidence excerpt." },
  { kind: "file", title: "Document preview", path: "docs/decisions.md", content: "# Project decisions\nKeep runtime authority in Rust." },
  { kind: "diff", title: "Proposed correction", before: "Review every proposal.", after: "Review every proposal before a distinct approval revision commits." },
  { kind: "terminal_output", title: "Validation output", command: "cargo test", output: "fixture: all selected tests passed", exitCode: 0 },
  { kind: "evaluation", title: "Evaluation evidence", status: "Fixture only. No promotion authorized.", metrics: [{ label: "Cases", value: "12" }, { label: "Outcome", value: "12 passed" }] },
];

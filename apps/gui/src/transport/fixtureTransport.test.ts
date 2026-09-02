import { describe, expect, it } from "vitest";
import type { ClientFrame } from "../protocol";
import { FixtureTransport } from "./fixtureTransport";

describe("fixture session lifecycle", () => {
  it("simulates rename, archive, unarchive, and deletion through authoritative projections", async () => {
    const transport = new FixtureTransport("ready");
    const frames: ClientFrame[] = [];
    await transport.connect((frame) => frames.push(frame));

    await transport.command({
      type: "rename_session",
      sessionId: "session-context",
      title: "Renamed context audit",
    });
    await transport.command({ type: "archive_session", sessionId: "session-context" });
    let snapshot = await transport.snapshot();
    expect(snapshot.sessions.find((session) => session.id === "session-context")).toMatchObject({
      title: "Renamed context audit",
      archived: true,
    });

    await transport.command({ type: "unarchive_session", sessionId: "session-context" });
    await transport.command({ type: "delete_session", sessionId: "session-context" });
    snapshot = await transport.snapshot();
    expect(snapshot.sessions.some((session) => session.id === "session-context")).toBe(false);
    expect(frames.some((frame) => frame.kind === "notice" && frame.code === "command_committed")).toBe(true);
  });

  it("selects another open session after deleting the active session", async () => {
    const transport = new FixtureTransport("ready");
    await transport.connect(() => undefined);

    await transport.command({ type: "delete_session", sessionId: "session-gui-migration" });

    const snapshot = await transport.snapshot();
    expect(snapshot.sessions.some((session) => session.id === "session-gui-migration")).toBe(false);
    expect(snapshot.activeSessionId).toBe("session-context");
    expect(snapshot.activeSession?.id).toBe("session-context");
  });
});

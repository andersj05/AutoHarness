import { afterEach, describe, expect, it, vi } from "vitest";
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

describe("fixture provider profiles", () => {
  afterEach(() => vi.useRealTimers());

  it("simulates router creation, vault save, activation, defaults, and content-free testing", async () => {
    vi.useFakeTimers();
    const transport = new FixtureTransport("ready");
    const frames: ClientFrame[] = [];
    await transport.connect((frame) => frames.push(frame));

    await transport.command({
      type: "upsert_provider_profile",
      profile: {
        id: "team-router",
        configuration: {
          kind: "router",
          baseUrl: "https://router.example.test/v1",
          project: "team",
          authHeader: "x-api-key",
        },
      },
    });
    await transport.submitCredential({
      connectionId: "team-router",
      operation: "save",
      credential: "fixture-only-secret",
    });
    await transport.command({ type: "activate_provider_profile", connectionId: "team-router" });
    await transport.command({
      type: "set_provider_defaults",
      connectionId: "team-router",
      modelId: "router/deepseek-v3.2",
      reasoningEffort: "medium",
    });
    const testReceipt = await transport.command({ type: "test_provider_profile", connectionId: "team-router" });
    await vi.runAllTimersAsync();

    const profile = (await transport.snapshot()).providers.find((candidate) => candidate.id === "team-router");
    expect(profile).toMatchObject({
      active: true,
      status: "ready",
      credentialSource: "vault",
      credentialState: "stored",
      defaultModelId: "router/deepseek-v3.2",
      defaultReasoningEffort: "medium",
    });
    expect(frames).toContainEqual(expect.objectContaining({
      kind: "notice",
      requestId: testReceipt.requestId,
      code: "command_committed",
    }));
  });

  it("simulates the native Codex browser authentication lifecycle", async () => {
    vi.useFakeTimers();
    const transport = new FixtureTransport("ready");
    const frames: ClientFrame[] = [];
    await transport.connect((frame) => frames.push(frame));

    const receipt = await transport.command({ type: "start_codex_authentication" });
    expect(frames).toContainEqual(expect.objectContaining({
      kind: "notice",
      requestId: receipt.requestId,
      code: "authentication_browser_opened",
    }));
    await vi.runAllTimersAsync();

    const snapshot = await transport.snapshot();
    expect(snapshot.providers.find((profile) => profile.configuration.kind === "codex_subscription")).toMatchObject({
      active: true,
      credentialSource: "vault",
      credentialState: "stored",
      status: "ready",
    });
    expect(frames).toContainEqual(expect.objectContaining({
      kind: "notice",
      requestId: receipt.requestId,
      code: "authentication_completed",
    }));
  });
});

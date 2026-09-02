import { describe, expect, it, vi } from "vitest";
import type { ClientCommand, ClientFrame, ClientSnapshot, ClientTransport, CommandReceipt, EphemeralCredential } from "../protocol";
import { createFixtureSnapshot } from "../transport/fixtureTransport";
import { ClientStore } from "./clientStore";

class TestTransport implements ClientTransport {
  listener?: (frame: ClientFrame) => void;
  errorListener?: (error: unknown) => void;
  snapshotCalls = 0;
  readonly commands: ClientCommand[] = [];
  readonly credentials: EphemeralCredential[] = [];
  baseline = createFixtureSnapshot("ready");
  resyncSnapshot: ClientSnapshot = { ...createFixtureSnapshot("ready"), transportRevision: "9" };

  async connect(listener: (frame: ClientFrame) => void, onError: (error: unknown) => void) {
    this.listener = listener;
    this.errorListener = onError;
    return structuredClone(this.baseline);
  }

  async command(command: ClientCommand): Promise<CommandReceipt> {
    this.commands.push(command);
    return { requestId: "1" };
  }

  async snapshot() {
    this.snapshotCalls += 1;
    return structuredClone(this.resyncSnapshot);
  }

  async submitCredential(secret: EphemeralCredential) {
    this.credentials.push({ ...secret });
    return { requestId: "2" };
  }

  async close() {}
}

class DeferredSnapshotTransport extends TestTransport {
  resolveSnapshot?: (snapshot: ClientSnapshot) => void;

  override async snapshot(): Promise<ClientSnapshot> {
    this.snapshotCalls += 1;
    return new Promise((resolve) => { this.resolveSnapshot = resolve; });
  }
}

class FailingConnectTransport extends TestTransport {
  override async connect(
    listener: (frame: ClientFrame) => void,
    onError: (error: unknown) => void,
  ): Promise<ClientSnapshot> {
    this.listener = listener;
    this.errorListener = onError;
    onError(new Error("Initial frame acknowledgement failed"));
    return structuredClone(this.baseline);
  }
}

describe("ClientStore", () => {
  it("does not overwrite a fatal connect callback with the returned baseline", async () => {
    const transport = new FailingConnectTransport();
    const store = new ClientStore(transport);

    await store.start();

    expect(store.getSnapshot()).toMatchObject({
      lifecycle: "failed",
      commandError: "Initial frame acknowledgement failed",
    });
    transport.listener?.({
      kind: "snapshot",
      reason: "projection",
      revision: "2",
      snapshot: { ...createFixtureSnapshot("ready"), transportRevision: "2" },
    });
    expect(store.getSnapshot().lifecycle).toBe("failed");
  });

  it("requests one authoritative resynchronization when an incremental revision is missing", async () => {
    const transport = new TestTransport();
    const store = new ClientStore(transport);
    await store.start();

    store.applyFrame({
      kind: "snapshot",
      reason: "projection",
      revision: "3",
      snapshot: { ...createFixtureSnapshot("ready"), transportRevision: "3" },
    });
    await vi.waitFor(() => expect(store.getSnapshot().transportRevision).toBe("9"));

    expect(transport.snapshotCalls).toBe(1);
    expect(store.getSnapshot().lifecycle).toBe("ready");
    expect(store.getSnapshot().notice?.code).toBe("projection_resynchronized");
  });

  it("accepts a newer resynchronization baseline across a revision gap", async () => {
    const transport = new TestTransport();
    const store = new ClientStore(transport);
    await store.start();

    store.applyFrame({
      kind: "snapshot",
      reason: "resynchronization",
      revision: "12",
      snapshot: { ...createFixtureSnapshot("offline"), transportRevision: "12" },
    });

    expect(transport.snapshotCalls).toBe(0);
    expect(store.getSnapshot().transportRevision).toBe("12");
    expect(store.getSnapshot().projection?.connection.kind).toBe("offline");
  });

  it("applies a bounded active-session splice without rebuilding unchanged rows", async () => {
    const transport = new TestTransport();
    const fixture = createFixtureSnapshot("ready");
    const history = Array.from({ length: 65_000 }, (_, index) => ({
      kind: "message" as const,
      id: `history-${String(index)}`,
      role: index % 2 === 0 ? "user" as const : "agent" as const,
      content: `Durable history row ${String(index)}`,
    }));
    transport.baseline = {
      ...fixture,
      sessions: fixture.sessions.map((session) => (
        session.id === fixture.activeSessionId ? { ...session, messageCount: "65000" } : session
      )),
      activeSession: { ...fixture.activeSession!, transcript: history },
    };
    const store = new ClientStore(transport);
    await store.start();
    const baseline = store.getSnapshot().projection!;
    const active = baseline.activeSession!;
    const transcriptReference = active.transcript;
    const firstRow = active.transcript[0];

    store.applyFrame({
      kind: "active_session_delta",
      revision: "2",
      sessionId: active.id,
      sessionRevision: "15",
      summary: { ...baseline.sessions[0]!, title: "Renamed while streaming", messageCount: "8" },
      selectedModelId: active.selectedModelId,
      transcript: {
        start: active.transcript.length - 1,
        deleteCount: 1,
        items: [{
          kind: "message",
          id: "attempt-delta",
          role: "agent",
          content: "Only this changed row crossed the carrier.",
          streaming: true,
        }],
      },
      attempt: { kind: "streaming", id: "attempt-delta", startedAt: "" },
    });

    const updated = store.getSnapshot().projection!;
    expect(updated.transportRevision).toBe("2");
    expect(updated.activeSession).toMatchObject({
      revision: "15",
      title: "Renamed while streaming",
      attempt: { kind: "streaming", id: "attempt-delta" },
    });
    expect(updated.activeSession?.transcript).toHaveLength(65_000);
    expect(updated.activeSession?.transcript).toBe(transcriptReference);
    expect(updated.activeSession?.transcript[0]).toBe(firstRow);
    expect(updated.activeSession?.transcript.at(-1)).toMatchObject({
      id: "attempt-delta",
      content: "Only this changed row crossed the carrier.",
    });
  });

  it("ignores stale frames after a baseline", async () => {
    const transport = new TestTransport();
    const store = new ClientStore(transport);
    await store.start();
    store.applyFrame({
      kind: "notice",
      revision: "1",
      level: "error",
      code: "stale",
      message: "stale",
    });
    expect(store.getSnapshot().notice).toBeUndefined();
  });

  it("does not roll back a newer increment when the resync dispatch receipt arrives later", async () => {
    const transport = new DeferredSnapshotTransport();
    const store = new ClientStore(transport);
    await store.start();
    store.applyFrame({
      kind: "snapshot",
      reason: "projection",
      revision: "3",
      snapshot: { ...createFixtureSnapshot("ready"), transportRevision: "3" },
    });
    store.applyFrame({
      kind: "snapshot",
      reason: "resynchronization",
      revision: "9",
      snapshot: { ...createFixtureSnapshot("ready"), transportRevision: "9" },
    });
    store.applyFrame({
      kind: "snapshot",
      reason: "projection",
      revision: "10",
      snapshot: { ...createFixtureSnapshot("streaming"), transportRevision: "10" },
    });
    transport.resolveSnapshot?.({ ...createFixtureSnapshot("ready"), transportRevision: "9" });
    await vi.waitFor(() => expect(store.getSnapshot().lifecycle).toBe("ready"));
    expect(store.getSnapshot().transportRevision).toBe("10");
    expect(store.getSnapshot().projection?.activeSession?.attempt.kind).toBe("streaming");
  });

  it("does not let a later initial frame skip a missing revision", async () => {
    const transport = new TestTransport();
    const store = new ClientStore(transport);
    await store.start();
    store.applyFrame({
      kind: "snapshot",
      reason: "initial",
      revision: "4",
      snapshot: { ...createFixtureSnapshot("ready"), transportRevision: "4" },
    });
    await vi.waitFor(() => expect(transport.snapshotCalls).toBe(1));
  });

  it("correlates a terminal command notice across a revision gap before resynchronizing", async () => {
    const transport = new DeferredSnapshotTransport();
    const store = new ClientStore(transport);
    await store.start();
    const committed = store.dispatchAndWait({ type: "create_session" });
    await Promise.resolve();
    await Promise.resolve();

    store.applyFrame({
      kind: "notice",
      revision: "3",
      requestId: "1",
      level: "success",
      code: "command_committed",
      message: "committed beyond the gap",
    });

    await expect(committed).resolves.toBe("committed");
    expect(transport.snapshotCalls).toBe(1);
    transport.resolveSnapshot?.({ ...createFixtureSnapshot("ready"), transportRevision: "9" });
    await vi.waitFor(() => expect(store.getSnapshot().transportRevision).toBe("9"));
  });

  it("fails closed when the transport reports an asynchronous frame decode error", async () => {
    const transport = new TestTransport();
    const store = new ClientStore(transport);
    await store.start();
    transport.errorListener?.(new Error("Invalid server frame"));
    expect(store.getSnapshot()).toMatchObject({ lifecycle: "failed", commandError: "Invalid server frame" });

    await expect(store.dispatch({ type: "create_session" })).resolves.toBeUndefined();
    await expect(store.submitCredential({ connectionId: "connection-gemini", credential: "secret" })).resolves.toBeUndefined();
    expect(transport.commands).toHaveLength(0);
    expect(transport.credentials).toHaveLength(0);

    await store.requestResync();
    expect(store.getSnapshot().lifecycle).toBe("ready");
    await expect(store.dispatch({ type: "create_session" })).resolves.toEqual({ requestId: "1" });
    expect(transport.commands).toHaveLength(1);
  });

  it("fails closed for a noncanonical transport revision", async () => {
    const transport = new TestTransport();
    const store = new ClientStore(transport);
    await store.start();
    store.applyFrame({ kind: "notice", revision: "01", level: "info", code: "bad", message: "bad" });
    expect(store.getSnapshot().lifecycle).toBe("failed");
  });

  it("keeps commands blocked while dependent frames arrive during recovery", async () => {
    const transport = new DeferredSnapshotTransport();
    const store = new ClientStore(transport);
    await store.start();

    const recovery = store.requestResync();
    expect(store.getSnapshot().lifecycle).toBe("resyncing");
    store.applyFrame({
      kind: "snapshot",
      reason: "projection",
      revision: "2",
      snapshot: { ...createFixtureSnapshot("ready"), transportRevision: "2" },
    });
    expect(store.getSnapshot().lifecycle).toBe("resyncing");
    store.applyFrame({
      kind: "notice",
      revision: "3",
      level: "info",
      code: "late_notice",
      message: "A dependent notice arrived before recovery completed",
    });
    expect(store.getSnapshot().lifecycle).toBe("resyncing");
    await expect(store.dispatch({ type: "create_session" })).resolves.toBeUndefined();

    transport.resolveSnapshot?.({ ...createFixtureSnapshot("ready"), transportRevision: "9" });
    await recovery;
    expect(store.getSnapshot().lifecycle).toBe("ready");
  });

  it("treats a missing terminal command result as unknown and resynchronizes", async () => {
    const transport = new TestTransport();
    const store = new ClientStore(transport, 1);
    await store.start();

    await expect(store.dispatchAndWait({ type: "create_session" })).resolves.toBe("unknown");
    await vi.waitFor(() => expect(transport.snapshotCalls).toBe(1));
    expect(store.getSnapshot().commandError).toBe("The host did not confirm whether the command committed.");
  });

  it("keeps a request-correlated optimistic prompt until authoritative observation", async () => {
    const transport = new TestTransport();
    const store = new ClientStore(transport);
    await store.start();
    const baseline = store.getSnapshot().projection!;

    const settlement = store.dispatchAndWait({
      type: "submit_prompt",
      sessionId: baseline.activeSessionId!,
      prompt: "Optimistic exact prompt",
    });
    await vi.waitFor(() => expect(store.getSnapshot().optimisticPrompts).toEqual([{
      requestId: "1",
      sessionId: baseline.activeSessionId,
      content: "Optimistic exact prompt",
    }]));

    store.applyFrame({
      kind: "notice",
      revision: "2",
      requestId: "1",
      level: "success",
      code: "command_committed",
      message: "committed",
    });
    await expect(settlement).resolves.toBe("committed");
    expect(store.getSnapshot().optimisticPrompts).toHaveLength(1);

    const activeSession = baseline.activeSession!;
    store.applyFrame({
      kind: "snapshot",
      reason: "projection",
      revision: "3",
      snapshot: {
        ...baseline,
        transportRevision: "3",
        activeSession: {
          ...activeSession,
          transcript: [
            ...activeSession.transcript,
            { kind: "message", id: "durable-input-new", role: "user", content: "Optimistic exact prompt" },
          ],
        },
      },
    });
    expect(store.getSnapshot().optimisticPrompts).toHaveLength(0);
  });

  it("retires an optimistic prompt on correlated rejection", async () => {
    const transport = new TestTransport();
    const store = new ClientStore(transport);
    await store.start();
    const sessionId = store.getSnapshot().projection!.activeSessionId!;
    const settlement = store.dispatchAndWait({ type: "submit_prompt", sessionId, prompt: "Rejected prompt" });
    await vi.waitFor(() => expect(store.getSnapshot().optimisticPrompts).toHaveLength(1));

    store.applyFrame({
      kind: "notice",
      revision: "2",
      requestId: "1",
      level: "error",
      code: "prompt_rejected",
      message: "rejected",
    });

    await expect(settlement).resolves.toBe("rejected");
    expect(store.getSnapshot().optimisticPrompts).toHaveLength(0);
  });
});

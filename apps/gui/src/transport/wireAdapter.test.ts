import { describe, expect, it } from "vitest";
import { commandToWire, frameFromWire, modelRefKey, receiptFromWire, snapshotFromWire } from "./wireAdapter";
import type { WireClientSnapshot, WireServerFrame } from "./wire";

function wireSnapshot(): WireClientSnapshot {
  return {
    schema_version: 1,
    lifecycle: { kind: "ready" },
    active_session_id: "session-1",
    sessions: [
      {
        session_id: "session-1",
        title: "Wire parity",
        revision: "4",
        selected_model: { provider_id: "gemini", model_id: "gemini-2.5-pro" },
        updated_at_ms: "1788091200000",
        message_count: "9007199254740993",
        archived: false,
      },
    ],
    active_session: {
      session_id: "session-1",
      revision: "4",
      selected_model: { provider_id: "gemini", model_id: "gemini-2.5-pro" },
      transcript: [
        {
          kind: "assistant",
          payload: {
            attempt_id: "attempt-1",
            content: "ready",
            state: { kind: "completed" },
            usage: {
              input_tokens: "41",
              output_tokens: "8",
              cached_input_tokens: null,
              reasoning_tokens: null,
              tool_tokens: null,
              total_tokens: "49",
            },
            retry_of: null,
          },
        },
      ],
      permission_requests: [],
    },
    catalog: {
      kind: "ready",
      payload: {
        generation: "7",
        stale: false,
        models: [
          {
            model: { provider_id: "gemini", model_id: "gemini-2.5-pro" },
            display_name: "Gemini 2.5 Pro",
            detail: "Reasoning model",
            context_window_tokens: "1048576",
            selectable: true,
            chat: "supported",
            streaming: "supported",
            thinking: "supported",
            tool_calling: "supported",
          },
        ],
      },
    },
    providers: [
      {
        connection_id: "profile-primary",
        provider_id: "gemini",
        display_name: "Primary Gemini",
        active: true,
        status: { kind: "ready" },
        credential_source: "vault",
        default_model: { provider_id: "gemini", model_id: "gemini-2.5-pro" },
      },
    ],
  };
}

describe("wire adapter", () => {
  it("emits the exact Rust command envelope and model identity", () => {
    const modelId = modelRefKey({ provider_id: "gemini", model_id: "gemini-2.5-pro" });
    expect(commandToWire({ type: "select_model", sessionId: "session-1", modelId })).toEqual({
      schema_version: 1,
      command: {
        kind: "select_model",
        payload: {
          session_id: "session-1",
          model: { provider_id: "gemini", model_id: "gemini-2.5-pro" },
        },
      },
    });
  });

  it("maps the complete session lifecycle without losing exact scope", () => {
    expect(commandToWire({ type: "rename_session", sessionId: "session-1", title: "Exact title" })).toEqual({
      schema_version: 1,
      command: {
        kind: "rename_session",
        payload: { session_id: "session-1", title: "Exact title" },
      },
    });
    expect(commandToWire({ type: "archive_session", sessionId: "session-2" }).command).toEqual({
      kind: "archive_session",
      payload: { session_id: "session-2" },
    });
    expect(commandToWire({ type: "unarchive_session", sessionId: "session-3" }).command).toEqual({
      kind: "unarchive_session",
      payload: { session_id: "session-3" },
    });
    expect(commandToWire({ type: "export_transcript", sessionId: "session-4" }).command).toEqual({
      kind: "export_transcript",
      payload: { session_id: "session-4" },
    });
    expect(commandToWire({ type: "delete_session", sessionId: "session-5" }).command).toEqual({
      kind: "delete_session",
      payload: { session_id: "session-5" },
    });
  });

  it("preserves exact decimal strings in the presentation projection", () => {
    const snapshot = snapshotFromWire(wireSnapshot(), "4");
    expect(snapshot.sessions[0]?.messageCount).toBe("9007199254740993");
    expect(snapshot.activeSession?.attempt).toMatchObject({ kind: "completed", inputTokens: "41", outputTokens: "8" });
    expect(snapshot.connectionId).toBe("profile-primary");
    expect(snapshot.runtimeMode).toBe("native");
    expect(snapshot.catalog.models[0]?.provider).toBe("Primary Gemini");
  });

  it("rejects noncanonical, zero, and out-of-range wire identities", () => {
    expect(() => receiptFromWire({ schema_version: 1, request_id: "01" })).toThrow(/request|decimal/i);
    expect(() => receiptFromWire({ schema_version: 1, request_id: "0" })).toThrow(/zero/i);
    expect(() => receiptFromWire({ schema_version: 1, request_id: "18446744073709551616" })).toThrow(/u64/i);
  });

  it("prefers an exact provider identity label before an unrelated active connection", () => {
    const original = wireSnapshot();
    const wire: WireClientSnapshot = {
      ...original,
      providers: [
        { ...original.providers[0]!, active: false, display_name: "Saved Gemini" },
        {
        connection_id: "profile-router",
        provider_id: "router",
        display_name: "Active Router",
        active: true,
        status: { kind: "ready" },
        credential_source: "vault",
        default_model: null,
        },
      ],
    };
    expect(snapshotFromWire(wire, "4").catalog.models[0]?.provider).toBe("Saved Gemini");
  });

  it("preserves unknown synthetic session metadata without inventing values", () => {
    const original = wireSnapshot();
    const wire: WireClientSnapshot = {
      ...original,
      sessions: [{ ...original.sessions[0]!, revision: null, updated_at_ms: null, message_count: null }],
    };
    expect(snapshotFromWire(wire, "4").sessions[0]).toMatchObject({
      updatedAt: undefined,
      messageCount: undefined,
    });
  });

  it("preserves provider credential requirements independently of a ready catalog", () => {
    const original = wireSnapshot();
    const wire: WireClientSnapshot = {
      ...original,
      providers: original.providers.map((provider) => ({
        ...provider,
        status: { kind: "credential_required" as const },
      })),
    };
    expect(snapshotFromWire(wire, "4").connection).toEqual({
      kind: "credential_required",
      providerLabel: "Primary Gemini",
      reason: "The active provider requires a credential before new model work can continue.",
    });
  });

  it("keeps tool denial transitional and preserves safe failure details", () => {
    const original = wireSnapshot();
    const toolBase = {
      tool_call_id: "tool-1",
      tool_name: "workspace.write",
      capability: "workspace.write",
      resource: "apps/gui/src/App.tsx",
      summary: "Update one file",
    };
    const wire: WireClientSnapshot = {
      ...original,
      active_session: {
        ...original.active_session!,
        transcript: [
          ...original.active_session!.transcript,
          { kind: "tool", payload: { ...toolBase, state: { kind: "denying" } } },
          {
            kind: "tool",
            payload: {
              ...toolBase,
              tool_call_id: "tool-2",
              state: {
                kind: "failed",
                payload: {
                  failure: {
                    class: "permission_denied",
                    code: "workspace_write_denied",
                    message: "The workspace rejected this exact write.",
                    retry: { kind: "never" },
                  },
                },
              },
            },
          },
        ],
      },
    };
    const tools = snapshotFromWire(wire, "4").activeSession?.transcript.filter((item) => item.kind === "tool");
    expect(tools?.[0]).toMatchObject({ status: "denying", detail: "denying" });
    expect(tools?.[1]).toMatchObject({
      status: "failed",
      failure: { code: "workspace_write_denied", message: "The workspace rejected this exact write." },
    });
  });

  it("preserves resynchronization reason on a server frame", () => {
    const frame: WireServerFrame = {
      schema_version: 1,
      revision: "22",
      payload: {
        kind: "snapshot",
        payload: { reason: "resynchronization", snapshot: wireSnapshot() },
      },
    };
    expect(frameFromWire(frame)).toMatchObject({ kind: "snapshot", reason: "resynchronization", revision: "22" });
  });

  it("maps only the changed active-session transcript rows", () => {
    const snapshot = wireSnapshot();
    const frame: WireServerFrame = {
      schema_version: 1,
      revision: "5",
      payload: {
        kind: "active_session_delta",
        payload: {
          session_id: "session-1",
          revision: "5",
          summary: { ...snapshot.sessions[0]!, revision: "5", message_count: "2" },
          selected_model: { provider_id: "gemini", model_id: "gemini-2.5-pro" },
          transcript: {
            start: 1,
            delete_count: 0,
            items: [{
              kind: "assistant",
              payload: {
                attempt_id: "attempt-2",
                content: "streamed suffix",
                state: { kind: "streaming" },
                usage: null,
                retry_of: null,
              },
            }],
          },
          permission_requests: [],
        },
      },
    };

    expect(frameFromWire(frame)).toMatchObject({
      kind: "active_session_delta",
      revision: "5",
      sessionId: "session-1",
      sessionRevision: "5",
      transcript: {
        start: 1,
        deleteCount: 0,
        items: [{ kind: "message", id: "attempt-2", content: "streamed suffix", streaming: true }],
      },
      attempt: { kind: "streaming", id: "attempt-2" },
    });
  });
});

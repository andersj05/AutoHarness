import { beforeEach, describe, expect, it, vi } from "vitest";

const carrier = vi.hoisted(() => ({ invoke: vi.fn() }));

vi.mock("@tauri-apps/api/core", () => {
  class Channel<T> {
    onmessage?: (message: T) => void;
  }
  return { Channel, invoke: carrier.invoke };
});

import { TauriTransport } from "./tauriTransport";

function initialFrame() {
  return {
    schema_version: 4,
    revision: "1",
    payload: {
      kind: "snapshot",
      payload: {
        reason: "initial",
        snapshot: {
          schema_version: 4,
          lifecycle: { kind: "ready" },
          active_session_id: null,
          sessions: [],
          active_session: null,
          catalog: { kind: "loading" },
          providers: [],
          settings: {
            theme_preset: { value: "system", source: "default", user_override: false },
            color_mode: { value: "color", source: "default", user_override: false },
            zoom_percent: { value: 100, source: "default", user_override: false },
            font_size: { value: "standard", source: "default", user_override: false },
            density: { value: "comfortable", source: "default", user_override: false },
            reduced_motion: { value: false, source: "default", user_override: false },
            timestamp_style: { value: "relative", source: "default", user_override: false },
            composer_submit_behavior: { value: "control_s", source: "default", user_override: false },
          },
          provider_recovery_pending: "0",
        },
      },
    },
  };
}

describe("TauriTransport", () => {
  beforeEach(() => {
    carrier.invoke.mockReset();
  });

  it("surfaces the host's safe restart guidance when a renderer cannot reconnect", async () => {
    carrier.invoke.mockRejectedValueOnce({
      code: "renderer_restart_required",
      message: "restart AutoHarness to recover the native renderer",
    });
    const transport = new TauriTransport();

    await expect(transport.connect(vi.fn(), vi.fn())).rejects.toThrow(
      "restart AutoHarness to recover the native renderer",
    );
  });

  it("rejects startup immediately and reports a malformed initial channel frame", async () => {
    carrier.invoke.mockImplementation(async (command: string, arguments_: { onFrame?: { onmessage?: (frame: unknown) => void } }) => {
      if (command === "gui_connect") {
        queueMicrotask(() => arguments_.onFrame?.onmessage?.({
          schema_version: 999,
          revision: "1",
          payload: { kind: "notice", payload: { kind: "shutdown", payload: { state: "ready" } } },
        }));
      }
    });
    const transport = new TauriTransport();
    const onError = vi.fn();

    await expect(transport.connect(vi.fn(), onError)).rejects.toThrow("Unsupported server frame schema");
    expect(onError).toHaveBeenCalledOnce();
    expect(onError.mock.calls[0]?.[0]).toBeInstanceOf(Error);
    expect(carrier.invoke).not.toHaveBeenCalledWith("gui_acknowledge_frame", expect.anything());
  });

  it("acknowledges a frame only after validation and synchronous application", async () => {
    const order: string[] = [];
    carrier.invoke.mockImplementation(async (command: string, arguments_: { onFrame?: { onmessage?: (frame: unknown) => void } }) => {
      if (command === "gui_connect") {
        queueMicrotask(() => arguments_.onFrame?.onmessage?.(initialFrame()));
      }
      if (command === "gui_acknowledge_frame") order.push("acknowledged");
    });
    const transport = new TauriTransport();
    const snapshot = await transport.connect(() => order.push("applied"), vi.fn());

    expect(snapshot.transportRevision).toBe("1");
    expect(order).toEqual(["applied", "acknowledged"]);
    expect(carrier.invoke).toHaveBeenCalledWith("gui_acknowledge_frame", { revision: "1" });
  });

  it("reports a disconnected command carrier as a fatal transport error", async () => {
    carrier.invoke.mockImplementation(async (command: string, arguments_: { onFrame?: { onmessage?: (frame: unknown) => void } }) => {
      if (command === "gui_connect") {
        queueMicrotask(() => arguments_.onFrame?.onmessage?.(initialFrame()));
        return;
      }
      if (command === "gui_dispatch") {
        throw { code: "host_disconnected", message: "the application host is no longer available" };
      }
    });
    const transport = new TauriTransport();
    const onError = vi.fn();
    await transport.connect(vi.fn(), onError);

    await expect(transport.command({ type: "refresh_catalog" })).rejects.toThrow(
      "the application host is no longer available",
    );
    expect(onError).toHaveBeenCalledOnce();
    expect(onError.mock.calls[0]?.[0]).toMatchObject({
      code: "host_disconnected",
      message: "the application host is no longer available",
    });
  });

  it("rejects a Rust-whitespace-only prompt without invoking or failing the carrier", async () => {
    carrier.invoke.mockImplementation(async (command: string, arguments_: { onFrame?: { onmessage?: (frame: unknown) => void } }) => {
      if (command === "gui_connect") {
        queueMicrotask(() => arguments_.onFrame?.onmessage?.(initialFrame()));
      }
    });
    const transport = new TauriTransport();
    const onError = vi.fn();
    await transport.connect(vi.fn(), onError);
    const invocationCount = carrier.invoke.mock.calls.length;

    await expect(transport.command({
      type: "submit_prompt",
      sessionId: "session-whitespace",
      prompt: "\u0085",
    })).rejects.toMatchObject({ code: "invalid_command" });

    expect(carrier.invoke).toHaveBeenCalledTimes(invocationCount);
    expect(onError).not.toHaveBeenCalled();
  });

  it("rejects only prompts beyond the Rust UTF-8 byte boundary without invoking or failing", async () => {
    carrier.invoke.mockImplementation(async (command: string, arguments_: { onFrame?: { onmessage?: (frame: unknown) => void } }) => {
      if (command === "gui_connect") {
        queueMicrotask(() => arguments_.onFrame?.onmessage?.(initialFrame()));
        return;
      }
      if (command === "gui_dispatch") {
        return { schema_version: 4, request_id: "1" };
      }
    });
    const transport = new TauriTransport();
    const onError = vi.fn();
    await transport.connect(vi.fn(), onError);
    const invokeCountBeforePrompts = carrier.invoke.mock.calls.length;
    const exactBoundary = "é".repeat(65_536);

    await expect(transport.command({
      type: "submit_prompt",
      sessionId: "session-boundary",
      prompt: exactBoundary,
    })).resolves.toEqual({ requestId: "1" });
    expect(carrier.invoke).toHaveBeenCalledTimes(invokeCountBeforePrompts + 1);

    await expect(transport.command({
      type: "submit_prompt",
      sessionId: "session-boundary",
      prompt: `${exactBoundary}a`,
    })).rejects.toMatchObject({ code: "invalid_command" });
    expect(carrier.invoke).toHaveBeenCalledTimes(invokeCountBeforePrompts + 1);
    expect(onError).not.toHaveBeenCalled();
  });

  it("forwards the non-secret credential operation through the dedicated ingress", async () => {
    carrier.invoke.mockResolvedValue({ schema_version: 4, request_id: "7" });
    const transport = new TauriTransport();

    await expect(transport.submitCredential({
      connectionId: "work-router",
      operation: "replace",
      credential: "replacement-secret",
    })).resolves.toEqual({ requestId: "7" });
    expect(carrier.invoke).toHaveBeenCalledWith("gui_submit_credential", {
      connectionId: "work-router",
      operation: "replace",
      credential: "replacement-secret",
    });
  });
});

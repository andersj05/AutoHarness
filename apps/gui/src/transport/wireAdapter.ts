import type {
  ActiveSessionProjection,
  AttemptProjection,
  CatalogProjection,
  ClientCommand,
  ClientFrame,
  ClientSnapshot,
  ConnectionState,
  ModelDescriptor,
  PermissionRequest,
  ToolMessage,
  TranscriptItem,
} from "../protocol";
import type {
  WireAttemptState,
  WireCatalogProjection,
  WireClientCommand,
  WireClientSnapshot,
  WireCommandEnvelope,
  WireCommandReceipt,
  WireModelRef,
  WireProviderProjection,
  WireServerFrame,
  WireToolCallProjection,
} from "./wire";
import { WIRE_SCHEMA_VERSION } from "./wire";

export function modelRefKey(model: WireModelRef): string {
  return `${encodeURIComponent(model.provider_id)}::${encodeURIComponent(model.model_id)}`;
}

export function modelRefFromKey(key: string): WireModelRef {
  const separator = key.indexOf("::");
  if (separator < 1) throw new Error("Invalid model reference");
  return {
    provider_id: decodeURIComponent(key.slice(0, separator)),
    model_id: decodeURIComponent(key.slice(separator + 2)),
  };
}

function retryable(state: WireAttemptState): boolean {
  return state.kind === "failed" && state.payload.failure.retry.kind !== "never";
}

const U64_MAX = 18_446_744_073_709_551_615n;
const I64_MAX = 9_223_372_036_854_775_807n;
const I64_MIN = -9_223_372_036_854_775_808n;

function decimalU64(value: string): string {
  if (!/^(0|[1-9]\d*)$/.test(value)) throw new Error("Host published a non-canonical unsigned decimal");
  const parsed = BigInt(value);
  if (parsed > U64_MAX) throw new Error("Host published an unsigned decimal outside u64 range");
  return value;
}

function positiveDecimalU64(value: string, field: string): string {
  decimalU64(value);
  if (value === "0") throw new Error(`Host published a zero ${field}`);
  return value;
}

function unixMillisToIso(value: string): string | undefined {
  if (!/^-?(0|[1-9]\d*)$/.test(value) || value === "-0") {
    throw new Error("Host published a non-canonical timestamp");
  }
  const parsed = BigInt(value);
  if (parsed < I64_MIN || parsed > I64_MAX) throw new Error("Host published a timestamp outside i64 range");
  const dateLimit = 8_640_000_000_000_000n;
  if (parsed > dateLimit || parsed < -dateLimit) return undefined;
  return new Date(Number(parsed)).toISOString();
}

function attemptFromWire(attemptId: string, state: WireAttemptState, usage: { input_tokens: string | null; output_tokens: string | null } | null): AttemptProjection {
  switch (state.kind) {
    case "streaming":
      return { kind: "streaming", id: attemptId, startedAt: "" };
    case "cancelling":
      return { kind: "cancelling", id: attemptId };
    case "completed":
      return {
        kind: "completed",
        id: attemptId,
        inputTokens: usage?.input_tokens ? decimalU64(usage.input_tokens) : undefined,
        outputTokens: usage?.output_tokens ? decimalU64(usage.output_tokens) : undefined,
      };
    case "cancelled":
      return { kind: "cancelled", id: attemptId };
    case "failed":
      return {
        kind: "failed",
        id: attemptId,
        code: state.payload.failure.code,
        message: state.payload.failure.message,
        retryable: retryable(state),
      };
  }
}

function toolFromWire(tool: WireToolCallProjection): ToolMessage {
  const status =
    tool.state.kind === "completed"
      ? "succeeded"
      : tool.state.kind === "denied"
        ? "denied"
        : tool.state.kind === "denying"
          ? "denying"
        : tool.state.kind === "failed" || tool.state.kind === "cancelled" || tool.state.kind === "unknown"
          ? "failed"
          : tool.state.kind === "running"
            ? "running"
            : "waiting";
  return {
    kind: "tool",
    id: tool.tool_call_id,
    name: tool.tool_name,
    summary: tool.summary ?? tool.capability,
    resource: tool.resource,
    status,
    detail: tool.state.kind === "failed" ? undefined : tool.state.kind.replaceAll("_", " "),
    failure: tool.state.kind === "failed"
      ? { code: tool.state.payload.failure.code, message: tool.state.payload.failure.message }
      : undefined,
  };
}

function activeSessionFromWire(snapshot: WireClientSnapshot, catalog: readonly ModelDescriptor[]): ActiveSessionProjection | undefined {
  const session = snapshot.active_session;
  if (!session) return undefined;
  let latestAttempt: AttemptProjection = { kind: "idle" };
  const transcript: TranscriptItem[] = session.transcript.map((item) => {
    if (item.kind === "user") {
      return { kind: "message", id: item.payload.input_id, role: "user", content: item.payload.content };
    }
    if (item.kind === "tool") return toolFromWire(item.payload);
    latestAttempt = attemptFromWire(item.payload.attempt_id, item.payload.state, item.payload.usage);
    return {
      kind: "message",
      id: item.payload.attempt_id,
      role: "agent",
      content: item.payload.content,
      streaming: item.payload.state.kind === "streaming" || item.payload.state.kind === "cancelling",
    };
  });
  const summary = snapshot.sessions.find((candidate) => candidate.session_id === session.session_id);
  const selectedModelId = session.selected_model ? modelRefKey(session.selected_model) : undefined;
  return {
    id: session.session_id,
    revision: session.revision,
    title: summary?.title ?? "Untitled session",
    selectedModelId,
    workspaceLabel: "Local workspace",
    transcript,
    attempt: latestAttempt,
  };
}

function catalogFromWire(catalog: WireCatalogProjection, providers: readonly WireProviderProjection[]): CatalogProjection {
  if (catalog.kind === "loading") return { status: "loading", source: "none", models: [] };
  if (catalog.kind === "credential_required") return { status: "credential_required", source: "none", models: [] };
  if (catalog.kind === "failed") {
    return { status: "failed", source: "none", models: [], safeError: catalog.payload.failure.message };
  }
  const activeProvider = providers.find((provider) => provider.active);
  return {
    status: catalog.payload.models.length > 0 ? "ready" : "empty",
    source: catalog.payload.stale ? "stale_cache" : "live",
    models: catalog.payload.models.map((model) => ({
      id: modelRefKey(model.model),
      displayName: model.display_name,
      provider:
        providers.find((provider) => provider.provider_id === model.model.provider_id && provider.active)?.display_name ??
        providers.find((provider) => provider.provider_id === model.model.provider_id)?.display_name ??
        activeProvider?.display_name ??
        "Configured provider",
      description: model.detail,
      contextWindowTokens: model.context_window_tokens ? decimalU64(model.context_window_tokens) : undefined,
      selectable: model.selectable,
      supportsTools: model.tool_calling === "supported" ? true : model.tool_calling === "unsupported" ? false : undefined,
      supportsReasoning: model.thinking === "supported" ? true : model.thinking === "unsupported" ? false : undefined,
    })),
  };
}

function connectionFromWire(snapshot: WireClientSnapshot): ConnectionState {
  const provider = snapshot.providers.find((candidate) => candidate.active) ?? snapshot.providers[0];
  if (snapshot.lifecycle.kind === "failed") {
    return { kind: "offline", reason: snapshot.lifecycle.payload.failure.message, recoverable: true };
  }
  if (snapshot.lifecycle.kind === "starting" || provider?.status.kind === "connecting") {
    return { kind: "connecting", providerLabel: provider?.display_name ?? "Provider" };
  }
  if (snapshot.lifecycle.kind === "ready" && provider?.status.kind === "ready") {
    const source = provider.credential_source.replaceAll("_", " ");
    return { kind: "online", providerLabel: provider.display_name, credentialSource: source };
  }
  if (provider?.status.kind === "credential_required") {
    return {
      kind: "credential_required",
      providerLabel: provider.display_name,
      reason: "The active provider requires a credential before new model work can continue.",
    };
  }
  const failure = provider?.status.kind === "failed" ? provider.status.payload.failure.message : undefined;
  return { kind: "offline", reason: failure ?? "The provider is unavailable. Durable replay remains available.", recoverable: true };
}

function permissionFromWire(snapshot: WireClientSnapshot): PermissionRequest | undefined {
  const permission = snapshot.active_session?.permission_requests[0];
  if (!permission || !snapshot.active_session) return undefined;
  return {
    id: permission.tool_call_id,
    sessionId: snapshot.active_session.session_id,
    toolName: permission.tool_name,
    capability: permission.capability,
    resource: permission.resource,
    reason: "The durable runtime requires your answer before this exact tool call can continue.",
    trustedFields: permission.details,
  };
}

export function snapshotFromWire(snapshot: WireClientSnapshot, revision: string): ClientSnapshot {
  if (snapshot.schema_version !== WIRE_SCHEMA_VERSION) throw new Error("Unsupported host snapshot schema");
  positiveDecimalU64(revision, "transport revision");
  snapshot.sessions.forEach((session) => {
    if (session.revision !== null) decimalU64(session.revision);
    if (session.message_count !== null) decimalU64(session.message_count);
  });
  if (snapshot.active_session) {
    decimalU64(snapshot.active_session.revision);
    snapshot.active_session.transcript.forEach((item) => {
      if (item.kind !== "assistant" || !item.payload.usage) return;
      Object.values(item.payload.usage).forEach((value) => {
        if (value !== null) decimalU64(value);
      });
    });
  }
  if (snapshot.catalog.kind === "ready") decimalU64(snapshot.catalog.payload.generation);
  const catalog = catalogFromWire(snapshot.catalog, snapshot.providers);
  const activeSession = activeSessionFromWire(snapshot, catalog.models);
  return {
    schemaVersion: WIRE_SCHEMA_VERSION,
    transportRevision: revision,
    runtimeMode: "native",
    connection: connectionFromWire(snapshot),
    connectionId: snapshot.providers.find((provider) => provider.active)?.connection_id,
    activeSessionId: snapshot.active_session_id ?? undefined,
    sessions: snapshot.sessions.map((session) => ({
      id: session.session_id,
      title: session.title,
      updatedAt: session.updated_at_ms === null ? undefined : unixMillisToIso(session.updated_at_ms),
      messageCount: session.message_count === null ? undefined : decimalU64(session.message_count),
      archived: session.archived,
    })),
    catalog,
    activeSession,
    pendingPermission: permissionFromWire(snapshot),
    activity: activeSession
      ? [
          { id: "replay", label: "Session projection", detail: `revision ${activeSession.revision}`, status: "complete" },
          {
            id: "attempt",
            label: "Provider turn",
            detail: activeSession.attempt.kind.replaceAll("_", " "),
            status:
              activeSession.attempt.kind === "streaming" || activeSession.attempt.kind === "cancelling"
                ? "active"
                : activeSession.attempt.kind === "failed"
                  ? "warning"
                  : "complete",
          },
        ]
      : [],
  };
}

export function commandToWire(command: ClientCommand): WireCommandEnvelope {
  let wire: WireClientCommand;
  switch (command.type) {
    case "create_session": wire = { kind: "create_session" }; break;
    case "open_session": wire = { kind: "open_session", payload: { session_id: command.sessionId } }; break;
    case "refresh_catalog": wire = { kind: "refresh_catalog" }; break;
    case "select_model": wire = { kind: "select_model", payload: { session_id: command.sessionId, model: modelRefFromKey(command.modelId) } }; break;
    case "submit_prompt": wire = { kind: "submit_prompt", payload: { session_id: command.sessionId, prompt: command.prompt } }; break;
    case "cancel_attempt": wire = { kind: "cancel_attempt", payload: { session_id: command.sessionId, attempt_id: command.attemptId } }; break;
    case "retry_attempt": wire = { kind: "retry_attempt", payload: { session_id: command.sessionId, attempt_id: command.attemptId } }; break;
    case "answer_permission": wire = { kind: "answer_permission", payload: { session_id: command.sessionId, tool_call_id: command.toolCallId, decision: command.decision } }; break;
  }
  return { schema_version: WIRE_SCHEMA_VERSION, command: wire };
}

export function receiptFromWire(receipt: WireCommandReceipt) {
  if (receipt.schema_version !== WIRE_SCHEMA_VERSION) throw new Error("Unsupported command receipt schema");
  return { requestId: positiveDecimalU64(receipt.request_id, "request id") };
}

export function frameFromWire(frame: WireServerFrame): ClientFrame {
  if (frame.schema_version !== WIRE_SCHEMA_VERSION) throw new Error("Unsupported server frame schema");
  const revision = positiveDecimalU64(frame.revision, "transport revision");
  if (frame.payload.kind === "snapshot") {
    return {
      kind: "snapshot",
      reason: frame.payload.payload.reason,
      revision,
      snapshot: snapshotFromWire(frame.payload.payload.snapshot, revision),
    };
  }
  const notice = frame.payload.payload;
  if (notice.kind === "command_rejected") {
    return { kind: "notice", revision, requestId: positiveDecimalU64(notice.payload.request_id, "request id"), level: "error", code: notice.payload.failure.code, message: notice.payload.failure.message };
  }
  if (notice.kind === "command_committed") {
    return { kind: "notice", revision, requestId: positiveDecimalU64(notice.payload.request_id, "request id"), level: "success", code: "command_committed", message: "The durable host committed the command." };
  }
  if (notice.kind === "authentication") {
    return { kind: "notice", revision, requestId: positiveDecimalU64(notice.payload.request_id, "request id"), level: "info", code: `authentication_${notice.payload.state}`, message: `Authentication ${notice.payload.state.replaceAll("_", " ")}.` };
  }
  return { kind: "notice", revision, level: "info", code: `shutdown_${notice.payload.state}`, message: `Shutdown ${notice.payload.state}.` };
}

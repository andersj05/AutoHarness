import type {
  ActiveSessionProjection,
  AttemptProjection,
  CatalogProjection,
  ClientCommand,
  ClientFrame,
  ClientPreferenceChange,
  ClientSettingsProjection,
  ClientSnapshot,
  ColorMode,
  ConnectionState,
  ModelDescriptor,
  PermissionRequest,
  ProviderProfile,
  SessionSummary,
  ToolMessage,
  TranscriptItem,
} from "../protocol";
import type {
  WireActiveSessionDelta,
  WireAttemptState,
  WireCatalogProjection,
  WireClientCommand,
  WireClientPreferenceChange,
  WireClientSettingsProjection,
  WireClientSnapshot,
  WireCommandEnvelope,
  WireCommandReceipt,
  WireModelRef,
  WirePermissionRequest,
  WireProviderProjection,
  WireSessionSummary,
  WireServerFrame,
  WireToolCallProjection,
  WireTranscriptItem,
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

function colorModeFromWire(value: WireClientSettingsProjection["color_mode"]["value"]): ColorMode {
  return value === "no_color" ? "no-color" : value === "high_contrast" ? "high-contrast" : value;
}

function colorModeToWire(value: ColorMode): WireClientSettingsProjection["color_mode"]["value"] {
  return value === "no-color" ? "no_color" : value === "high-contrast" ? "high_contrast" : value;
}

function settingsFromWire(settings: WireClientSettingsProjection): ClientSettingsProjection {
  const zoomPercent = settings.zoom_percent.value;
  if (!Number.isInteger(zoomPercent) || zoomPercent < 75 || zoomPercent > 200) {
    throw new Error("Host published a GUI zoom outside the supported range");
  }
  return {
    themePreset: {
      value: settings.theme_preset.value,
      source: settings.theme_preset.source,
      userOverride: settings.theme_preset.user_override,
    },
    colorMode: {
      value: colorModeFromWire(settings.color_mode.value),
      source: settings.color_mode.source,
      userOverride: settings.color_mode.user_override,
    },
    zoomPercent: {
      value: zoomPercent,
      source: settings.zoom_percent.source,
      userOverride: settings.zoom_percent.user_override,
    },
    fontSize: {
      value: settings.font_size.value,
      source: settings.font_size.source,
      userOverride: settings.font_size.user_override,
    },
    density: {
      value: settings.density.value,
      source: settings.density.source,
      userOverride: settings.density.user_override,
    },
    reducedMotion: {
      value: settings.reduced_motion.value,
      source: settings.reduced_motion.source,
      userOverride: settings.reduced_motion.user_override,
    },
    timestampStyle: {
      value: settings.timestamp_style.value,
      source: settings.timestamp_style.source,
      userOverride: settings.timestamp_style.user_override,
    },
    composerSubmitBehavior: {
      value: settings.composer_submit_behavior.value,
      source: settings.composer_submit_behavior.source,
      userOverride: settings.composer_submit_behavior.user_override,
    },
  };
}

function preferenceChangeToWire(change: ClientPreferenceChange): WireClientPreferenceChange {
  switch (change.kind) {
    case "theme_preset":
      return { kind: change.kind, payload: { value: change.value } };
    case "color_mode":
      return { kind: change.kind, payload: { value: change.value === null ? null : colorModeToWire(change.value) } };
    case "zoom_percent":
      if (change.value !== null && (!Number.isInteger(change.value) || change.value < 75 || change.value > 200)) {
        throw new Error("GUI zoom must be a whole percentage from 75 through 200");
      }
      return { kind: change.kind, payload: { value: change.value } };
    case "font_size":
      return { kind: change.kind, payload: { value: change.value } };
    case "density":
      return { kind: change.kind, payload: { value: change.value } };
    case "reduced_motion":
      return { kind: change.kind, payload: { value: change.value } };
    case "timestamp_style":
      return { kind: change.kind, payload: { value: change.value } };
    case "composer_submit_behavior":
      return { kind: change.kind, payload: { value: change.value } };
  }
}

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

function transcriptItemFromWire(item: WireTranscriptItem): {
  item: TranscriptItem;
  attempt?: AttemptProjection;
} {
  if (item.kind === "user") {
    return {
      item: { kind: "message", id: item.payload.input_id, role: "user", content: item.payload.content },
    };
  }
  if (item.kind === "tool") return { item: toolFromWire(item.payload) };
  const attempt = attemptFromWire(item.payload.attempt_id, item.payload.state, item.payload.usage);
  return {
    attempt,
    item: {
      kind: "message",
      id: item.payload.attempt_id,
      role: "agent",
      content: item.payload.content,
      streaming: item.payload.state.kind === "streaming" || item.payload.state.kind === "cancelling",
    },
  };
}

function sessionSummaryFromWire(session: WireSessionSummary): SessionSummary {
  if (session.revision !== null) decimalU64(session.revision);
  if (session.message_count !== null) decimalU64(session.message_count);
  return {
    id: session.session_id,
    title: session.title,
    updatedAt: session.updated_at_ms === null ? undefined : unixMillisToIso(session.updated_at_ms),
    messageCount: session.message_count === null ? undefined : decimalU64(session.message_count),
    archived: session.archived,
  };
}

function permissionRequestFromWire(
  permission: WirePermissionRequest,
  sessionId: string,
): PermissionRequest {
  return {
    id: permission.tool_call_id,
    sessionId,
    toolName: permission.tool_name,
    capability: permission.capability,
    resource: permission.resource,
    reason: "The durable runtime requires your answer before this exact tool call can continue.",
    trustedFields: permission.details,
  };
}

function activeSessionFromWire(snapshot: WireClientSnapshot, catalog: readonly ModelDescriptor[]): ActiveSessionProjection | undefined {
  const session = snapshot.active_session;
  if (!session) return undefined;
  let latestAttempt: AttemptProjection = { kind: "idle" };
  const transcript: TranscriptItem[] = session.transcript.map((item) => {
    const mapped = transcriptItemFromWire(item);
    if (mapped.attempt) latestAttempt = mapped.attempt;
    return mapped.item;
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

function providerFromWire(provider: WireProviderProjection): ProviderProfile {
  return {
    id: provider.connection_id,
    providerId: provider.provider_id,
    displayName: provider.display_name,
    configuration: {
      kind: provider.configuration.kind,
      baseUrl: provider.configuration.base_url ?? undefined,
      project: provider.configuration.project ?? undefined,
      authHeader: provider.configuration.auth_header ?? undefined,
    },
    scope: provider.scope,
    active: provider.active,
    status: provider.status.kind,
    safeError: provider.status.kind === "failed" ? provider.status.payload.failure.message : undefined,
    credentialSource: provider.credential_source,
    credentialState: provider.credential_state,
    defaultModelId: provider.default_model ? modelRefKey(provider.default_model) : undefined,
    defaultReasoningEffort: provider.default_reasoning_effort ?? undefined,
  };
}

function permissionFromWire(snapshot: WireClientSnapshot): PermissionRequest | undefined {
  const permission = snapshot.active_session?.permission_requests[0];
  if (!permission || !snapshot.active_session) return undefined;
  return permissionRequestFromWire(permission, snapshot.active_session.session_id);
}

export function snapshotFromWire(snapshot: WireClientSnapshot, revision: string): ClientSnapshot {
  if (snapshot.schema_version !== WIRE_SCHEMA_VERSION) throw new Error("Unsupported host snapshot schema");
  positiveDecimalU64(revision, "transport revision");
  snapshot.sessions.forEach(sessionSummaryFromWire);
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
  decimalU64(snapshot.provider_recovery_pending);
  const catalog = catalogFromWire(snapshot.catalog, snapshot.providers);
  const activeSession = activeSessionFromWire(snapshot, catalog.models);
  return {
    schemaVersion: WIRE_SCHEMA_VERSION,
    transportRevision: revision,
    runtimeMode: "native",
    connection: connectionFromWire(snapshot),
    connectionId: snapshot.providers.find((provider) => provider.active)?.connection_id,
    activeSessionId: snapshot.active_session_id ?? undefined,
    sessions: snapshot.sessions.map(sessionSummaryFromWire),
    catalog,
    providers: snapshot.providers.map(providerFromWire),
    settings: settingsFromWire(snapshot.settings),
    memory: snapshot.memory,
    providerRecoveryPending: snapshot.provider_recovery_pending,
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

function activeSessionDeltaFromWire(
  delta: WireActiveSessionDelta,
  revision: string,
): Extract<ClientFrame, { kind: "active_session_delta" }> {
  decimalU64(delta.revision);
  if (
    !Number.isSafeInteger(delta.transcript.start)
    || delta.transcript.start < 0
    || delta.transcript.start > 65_536
    || !Number.isSafeInteger(delta.transcript.delete_count)
    || delta.transcript.delete_count < 0
    || delta.transcript.delete_count > 65_536
    || delta.transcript.items.length > 65_536
  ) {
    throw new Error("Host published an invalid transcript delta range");
  }
  let attempt: AttemptProjection | undefined;
  const items = delta.transcript.items.map((item) => {
    if (item.kind === "assistant" && item.payload.usage) {
      Object.values(item.payload.usage).forEach((value) => {
        if (value !== null) decimalU64(value);
      });
    }
    const mapped = transcriptItemFromWire(item);
    if (mapped.attempt) attempt = mapped.attempt;
    return mapped.item;
  });
  const pending = delta.permission_requests[0];
  return {
    kind: "active_session_delta",
    revision,
    sessionId: delta.session_id,
    sessionRevision: delta.revision,
    summary: sessionSummaryFromWire(delta.summary),
    selectedModelId: delta.selected_model ? modelRefKey(delta.selected_model) : undefined,
    transcript: {
      start: delta.transcript.start,
      deleteCount: delta.transcript.delete_count,
      items,
    },
    attempt,
    pendingPermission: pending ? permissionRequestFromWire(pending, delta.session_id) : undefined,
  };
}

export function commandToWire(command: ClientCommand): WireCommandEnvelope {
  let wire: WireClientCommand;
  switch (command.type) {
    case "memory": wire = { kind: "memory", payload: { command: command.command } }; break;
    case "create_session": wire = { kind: "create_session" }; break;
    case "open_session": wire = { kind: "open_session", payload: { session_id: command.sessionId } }; break;
    case "rename_session": wire = { kind: "rename_session", payload: { session_id: command.sessionId, title: command.title } }; break;
    case "archive_session": wire = { kind: "archive_session", payload: { session_id: command.sessionId } }; break;
    case "unarchive_session": wire = { kind: "unarchive_session", payload: { session_id: command.sessionId } }; break;
    case "export_transcript": wire = { kind: "export_transcript", payload: { session_id: command.sessionId } }; break;
    case "delete_session": wire = { kind: "delete_session", payload: { session_id: command.sessionId } }; break;
    case "upsert_provider_profile": wire = {
      kind: "upsert_provider_profile",
      payload: {
        profile: {
          connection_id: command.profile.id,
          configuration: {
            kind: command.profile.configuration.kind,
            base_url: command.profile.configuration.baseUrl ?? null,
            project: command.profile.configuration.project ?? null,
            auth_header: command.profile.configuration.authHeader ?? null,
          },
        },
      },
    }; break;
    case "duplicate_provider_profile": wire = {
      kind: "duplicate_provider_profile",
      payload: {
        source_connection_id: command.sourceId,
        destination_connection_id: command.destinationId,
      },
    }; break;
    case "activate_provider_profile": wire = { kind: "activate_provider_profile", payload: { connection_id: command.connectionId } }; break;
    case "test_provider_profile": wire = { kind: "test_provider_profile", payload: { connection_id: command.connectionId } }; break;
    case "set_provider_defaults": wire = {
      kind: "set_provider_defaults",
      payload: {
        connection_id: command.connectionId,
        model: modelRefFromKey(command.modelId),
        reasoning_effort: command.reasoningEffort ?? null,
      },
    }; break;
    case "disconnect_provider_profile": wire = { kind: "disconnect_provider_profile", payload: { connection_id: command.connectionId } }; break;
    case "delete_provider_profile": wire = { kind: "delete_provider_profile", payload: { connection_id: command.connectionId } }; break;
    case "update_client_preference": wire = {
      kind: "update_client_preference",
      payload: { change: preferenceChangeToWire(command.change) },
    }; break;
    case "start_codex_authentication": wire = { kind: "start_codex_authentication" }; break;
    case "cancel_codex_authentication": wire = {
      kind: "cancel_codex_authentication",
      payload: { authentication_request_id: command.authenticationRequestId },
    }; break;
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
  if (frame.payload.kind === "active_session_delta") {
    return activeSessionDeltaFromWire(frame.payload.payload, revision);
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

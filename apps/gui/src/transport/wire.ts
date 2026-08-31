/* Exact schema-v1 JSON surface from crates/autoharness-client. */

export const WIRE_SCHEMA_VERSION = 1 as const;

export interface WireModelRef {
  provider_id: string;
  model_id: string;
}

export type WireRetryDirective =
  | { kind: "never" }
  | { kind: "immediate" }
  | { kind: "after"; payload: { delay_ms: string } };

export interface WireSafeFailure {
  class:
    | "validation"
    | "not_found"
    | "conflict"
    | "authentication"
    | "permission_denied"
    | "rate_limited"
    | "timeout"
    | "unavailable"
    | "cancelled"
    | "protocol"
    | "storage"
    | "internal";
  code: string;
  message: string;
  retry: WireRetryDirective;
}

export interface WireUsageProjection {
  input_tokens: string | null;
  output_tokens: string | null;
  cached_input_tokens: string | null;
  reasoning_tokens: string | null;
  tool_tokens: string | null;
  total_tokens: string | null;
}

export type WireAttemptState =
  | { kind: "streaming" }
  | { kind: "cancelling" }
  | { kind: "completed" }
  | { kind: "cancelled" }
  | { kind: "failed"; payload: { failure: WireSafeFailure } };

export type WireToolCallState =
  | { kind: "proposed" }
  | { kind: "permission_pending" }
  | { kind: "authorized" }
  | { kind: "denying" }
  | { kind: "running" }
  | { kind: "completed" }
  | { kind: "failed"; payload: { failure: WireSafeFailure } }
  | { kind: "denied" }
  | { kind: "cancelled" }
  | { kind: "unknown" };

export interface WireToolCallProjection {
  tool_call_id: string;
  tool_name: string;
  capability: string;
  resource: string;
  state: WireToolCallState;
  summary: string | null;
}

export type WireTranscriptItem =
  | { kind: "user"; payload: { input_id: string; content: string } }
  | {
      kind: "assistant";
      payload: {
        attempt_id: string;
        content: string;
        state: WireAttemptState;
        usage: WireUsageProjection | null;
        retry_of: string | null;
      };
    }
  | { kind: "tool"; payload: WireToolCallProjection };

export interface WirePermissionRequest {
  tool_call_id: string;
  tool_name: string;
  capability: string;
  resource: string;
  details: readonly { label: string; value: string }[];
}

export interface WireSessionProjection {
  session_id: string;
  revision: string;
  selected_model: WireModelRef | null;
  transcript: readonly WireTranscriptItem[];
  permission_requests: readonly WirePermissionRequest[];
}

export interface WireSessionSummary {
  session_id: string;
  title: string;
  revision: string | null;
  selected_model: WireModelRef | null;
  updated_at_ms: string | null;
  message_count: string | null;
  archived: boolean;
}

export type WireCapabilitySupport = "supported" | "unsupported" | "unknown";

export interface WireModelSummary {
  model: WireModelRef;
  display_name: string;
  detail: string;
  context_window_tokens: string | null;
  selectable: boolean;
  chat: WireCapabilitySupport;
  streaming: WireCapabilitySupport;
  thinking: WireCapabilitySupport;
  tool_calling: WireCapabilitySupport;
}

export type WireCatalogProjection =
  | { kind: "credential_required" }
  | { kind: "loading" }
  | { kind: "ready"; payload: { generation: string; models: readonly WireModelSummary[]; stale: boolean } }
  | { kind: "failed"; payload: { failure: WireSafeFailure } };

export type WireProviderStatus =
  | { kind: "disconnected" }
  | { kind: "credential_required" }
  | { kind: "connecting" }
  | { kind: "ready" }
  | { kind: "offline" }
  | { kind: "failed"; payload: { failure: WireSafeFailure } };

export interface WireProviderProjection {
  connection_id: string;
  provider_id: string;
  display_name: string;
  active: boolean;
  status: WireProviderStatus;
  credential_source: "none" | "environment" | "vault" | "session_only";
  default_model: WireModelRef | null;
}

export type WireClientLifecycle =
  | { kind: "starting" }
  | { kind: "ready" }
  | { kind: "offline" }
  | { kind: "shutting_down" }
  | { kind: "failed"; payload: { failure: WireSafeFailure } };

export interface WireClientSnapshot {
  schema_version: typeof WIRE_SCHEMA_VERSION;
  lifecycle: WireClientLifecycle;
  active_session_id: string | null;
  sessions: readonly WireSessionSummary[];
  active_session: WireSessionProjection | null;
  catalog: WireCatalogProjection;
  providers: readonly WireProviderProjection[];
}

export type WireClientCommand =
  | { kind: "create_session" }
  | { kind: "open_session"; payload: { session_id: string } }
  | { kind: "refresh_catalog" }
  | { kind: "select_model"; payload: { session_id: string; model: WireModelRef } }
  | { kind: "submit_prompt"; payload: { session_id: string; prompt: string } }
  | { kind: "cancel_attempt"; payload: { session_id: string; attempt_id: string } }
  | { kind: "retry_attempt"; payload: { session_id: string; attempt_id: string } }
  | {
      kind: "answer_permission";
      payload: { session_id: string; tool_call_id: string; decision: "allow_once" | "deny" };
    }
  | { kind: "request_resynchronization"; payload: { last_applied_revision: string | null } }
  | { kind: "request_shutdown" };

export interface WireCommandEnvelope {
  schema_version: typeof WIRE_SCHEMA_VERSION;
  command: WireClientCommand;
}

export interface WireCommandReceipt {
  schema_version: typeof WIRE_SCHEMA_VERSION;
  request_id: string;
}

export type WireClientNotice =
  | { kind: "command_committed"; payload: { request_id: string } }
  | { kind: "command_rejected"; payload: { request_id: string; failure: WireSafeFailure } }
  | {
      kind: "authentication";
      payload: { request_id: string; state: "browser_opened" | "completed" | "cancelled" };
    }
  | { kind: "shutdown"; payload: { state: "requested" | "ready" } };

export type WireFramePayload =
  | {
      kind: "snapshot";
      payload: {
        reason: "initial" | "projection" | "resynchronization";
        snapshot: WireClientSnapshot;
      };
    }
  | { kind: "notice"; payload: WireClientNotice };

export interface WireServerFrame {
  schema_version: typeof WIRE_SCHEMA_VERSION;
  revision: string;
  payload: WireFramePayload;
}

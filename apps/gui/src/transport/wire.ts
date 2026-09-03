/* Exact schema-v3 JSON surface from crates/autoharness-client. */

export const WIRE_SCHEMA_VERSION = 3 as const;

export type WirePreferenceSource = "default" | "user_file" | "workspace_file" | "environment" | "command_line";

export interface WireEffectiveSetting<T> {
  value: T;
  source: WirePreferenceSource;
  user_override: boolean;
}

export interface WireClientSettingsProjection {
  theme_preset: WireEffectiveSetting<"system" | "light" | "dark" | "aurora" | "ember" | "midnight" | "ocean" | "forest" | "rose">;
  color_mode: WireEffectiveSetting<"color" | "soft" | "vivid" | "no_color" | "high_contrast">;
  zoom_percent: WireEffectiveSetting<number>;
  font_size: WireEffectiveSetting<"small" | "standard" | "large" | "extra_large">;
  density: WireEffectiveSetting<"comfortable" | "compact">;
  reduced_motion: WireEffectiveSetting<boolean>;
  timestamp_style: WireEffectiveSetting<"relative" | "absolute" | "hidden">;
  composer_submit_behavior: WireEffectiveSetting<"control_s" | "enter">;
}

export type WireClientPreferenceChange =
  | { kind: "theme_preset"; payload: { value: WireClientSettingsProjection["theme_preset"]["value"] | null } }
  | { kind: "color_mode"; payload: { value: WireClientSettingsProjection["color_mode"]["value"] | null } }
  | { kind: "zoom_percent"; payload: { value: number | null } }
  | { kind: "font_size"; payload: { value: WireClientSettingsProjection["font_size"]["value"] | null } }
  | { kind: "density"; payload: { value: WireClientSettingsProjection["density"]["value"] | null } }
  | { kind: "reduced_motion"; payload: { value: boolean | null } }
  | { kind: "timestamp_style"; payload: { value: WireClientSettingsProjection["timestamp_style"]["value"] | null } }
  | { kind: "composer_submit_behavior"; payload: { value: WireClientSettingsProjection["composer_submit_behavior"]["value"] | null } };

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
  | { kind: "untested" }
  | { kind: "connecting" }
  | { kind: "ready" }
  | { kind: "offline" }
  | { kind: "failed"; payload: { failure: WireSafeFailure } };

export interface WireProviderProjection {
  connection_id: string;
  provider_id: string;
  display_name: string;
  configuration: {
    kind: "gemini" | "router" | "codex_subscription";
    base_url: string | null;
    project: string | null;
    auth_header: string | null;
  };
  scope: "named" | "session_default";
  active: boolean;
  status: WireProviderStatus;
  credential_source: "none" | "environment" | "vault" | "session_only";
  credential_state: "disconnected" | "stored" | "recovery_pending";
  default_model: WireModelRef | null;
  default_reasoning_effort: "none" | "minimal" | "low" | "medium" | "high" | "xhigh" | "max" | null;
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
  settings: WireClientSettingsProjection;
  provider_recovery_pending: string;
}

export interface WireActiveSessionDelta {
  session_id: string;
  revision: string;
  summary: WireSessionSummary;
  selected_model: WireModelRef | null;
  transcript: {
    start: number;
    delete_count: number;
    items: readonly WireTranscriptItem[];
  };
  permission_requests: readonly WirePermissionRequest[];
}

export type WireClientCommand =
  | { kind: "create_session" }
  | { kind: "open_session"; payload: { session_id: string } }
  | { kind: "rename_session"; payload: { session_id: string; title: string } }
  | { kind: "archive_session"; payload: { session_id: string } }
  | { kind: "unarchive_session"; payload: { session_id: string } }
  | { kind: "export_transcript"; payload: { session_id: string } }
  | { kind: "delete_session"; payload: { session_id: string } }
  | {
      kind: "upsert_provider_profile";
      payload: {
        profile: {
          connection_id: string;
          configuration: {
            kind: "gemini" | "router" | "codex_subscription";
            base_url: string | null;
            project: string | null;
            auth_header: string | null;
          };
        };
      };
    }
  | {
      kind: "duplicate_provider_profile";
      payload: { source_connection_id: string; destination_connection_id: string };
    }
  | { kind: "activate_provider_profile"; payload: { connection_id: string } }
  | { kind: "test_provider_profile"; payload: { connection_id: string } }
  | {
      kind: "set_provider_defaults";
      payload: {
        connection_id: string;
        model: WireModelRef;
        reasoning_effort: "none" | "minimal" | "low" | "medium" | "high" | "xhigh" | "max" | null;
      };
    }
  | { kind: "disconnect_provider_profile"; payload: { connection_id: string } }
  | { kind: "delete_provider_profile"; payload: { connection_id: string } }
  | { kind: "update_client_preference"; payload: { change: WireClientPreferenceChange } }
  | { kind: "start_codex_authentication" }
  | {
      kind: "cancel_codex_authentication";
      payload: { authentication_request_id: string };
    }
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
  | { kind: "active_session_delta"; payload: WireActiveSessionDelta }
  | { kind: "notice"; payload: WireClientNotice };

export interface WireServerFrame {
  schema_version: typeof WIRE_SCHEMA_VERSION;
  revision: string;
  payload: WireFramePayload;
}

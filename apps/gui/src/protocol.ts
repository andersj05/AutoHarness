export const CLIENT_SCHEMA_VERSION = 3 as const;
export const MAX_PROMPT_UTF8_BYTES = 128 * 1024;
export const MAX_SESSION_TITLE_UTF8_BYTES = 128;

export type SessionId = string;
export type AttemptId = string;
export type ModelId = string;
export type RequestId = string;
export type Revision = string;
export type CommandOutcome = "committed" | "rejected" | "unknown";
export type ProviderKind = "gemini" | "router" | "codex_subscription";
export type ReasoningEffort = "none" | "minimal" | "low" | "medium" | "high" | "xhigh" | "max";
export type CredentialSource = "none" | "environment" | "vault" | "session_only";
export type ProviderCredentialState = "disconnected" | "stored" | "recovery_pending";
export type ProviderProfileScope = "named" | "session_default";
export type ProviderStatus = "disconnected" | "credential_required" | "untested" | "connecting" | "ready" | "offline" | "failed";
export type ThemePreset = "system" | "light" | "dark" | "aurora" | "ember" | "midnight" | "ocean" | "forest" | "rose";
export type ColorMode = "color" | "soft" | "vivid" | "no-color" | "high-contrast";
export type Density = "comfortable" | "compact";
export type TimestampStyle = "relative" | "absolute" | "hidden";
export type ComposerSubmitBehavior = "control_s" | "enter";
export type GuiFontSize = "small" | "standard" | "large" | "extra_large";
export type PreferenceSource = "default" | "user_file" | "workspace_file" | "environment" | "command_line";

export interface EffectiveSetting<T> {
  value: T;
  source: PreferenceSource;
  userOverride: boolean;
}

export interface ClientSettingsProjection {
  themePreset: EffectiveSetting<ThemePreset>;
  colorMode: EffectiveSetting<ColorMode>;
  zoomPercent: EffectiveSetting<number>;
  fontSize: EffectiveSetting<GuiFontSize>;
  density: EffectiveSetting<Density>;
  reducedMotion: EffectiveSetting<boolean>;
  timestampStyle: EffectiveSetting<TimestampStyle>;
  composerSubmitBehavior: EffectiveSetting<ComposerSubmitBehavior>;
}

export type ClientPreferenceChange =
  | { kind: "theme_preset"; value: ThemePreset | null }
  | { kind: "color_mode"; value: ColorMode | null }
  | { kind: "zoom_percent"; value: number | null }
  | { kind: "font_size"; value: GuiFontSize | null }
  | { kind: "density"; value: Density | null }
  | { kind: "reduced_motion"; value: boolean | null }
  | { kind: "timestamp_style"; value: TimestampStyle | null }
  | { kind: "composer_submit_behavior"; value: ComposerSubmitBehavior | null };

export type ConnectionState =
  | { kind: "online"; providerLabel: string; credentialSource: string }
  | { kind: "credential_required"; providerLabel: string; reason: string }
  | { kind: "offline"; reason: string; recoverable: boolean }
  | { kind: "connecting"; providerLabel: string };

export interface SessionSummary {
  id: SessionId;
  title: string;
  updatedAt?: string;
  messageCount?: string;
  archived: boolean;
}

export interface ModelDescriptor {
  id: ModelId;
  displayName: string;
  provider: string;
  description: string;
  contextWindowTokens?: string;
  selectable: boolean;
  supportsTools?: boolean;
  supportsReasoning?: boolean;
}

export interface CatalogProjection {
  status: "credential_required" | "loading" | "ready" | "empty" | "failed";
  source: "live" | "fresh_cache" | "stale_cache" | "none";
  models: readonly ModelDescriptor[];
  refreshedAt?: string;
  safeError?: string;
}

export type AttemptProjection =
  | { kind: "idle" }
  | { kind: "streaming"; id: AttemptId; startedAt: string }
  | { kind: "cancelling"; id: AttemptId }
  | { kind: "completed"; id: AttemptId; inputTokens?: string; outputTokens?: string }
  | { kind: "cancelled"; id: AttemptId }
  | { kind: "failed"; id: AttemptId; code: string; message: string; retryable: boolean };

export interface TextMessage {
  kind: "message";
  id: string;
  role: "user" | "agent";
  content: string;
  createdAt?: string;
  streaming?: boolean;
}

export interface ToolMessage {
  kind: "tool";
  id: string;
  name: string;
  summary: string;
  resource: string;
  status: "waiting" | "running" | "denying" | "succeeded" | "denied" | "failed";
  detail?: string;
  failure?: { code: string; message: string };
}

export type TranscriptItem = TextMessage | ToolMessage;

export interface ActiveSessionProjection {
  id: SessionId;
  revision: Revision;
  title: string;
  selectedModelId?: ModelId;
  workspaceLabel: string;
  branchLabel?: string;
  transcript: readonly TranscriptItem[];
  attempt: AttemptProjection;
}

export interface PermissionRequest {
  id: string;
  sessionId: SessionId;
  toolName: string;
  capability: string;
  resource: string;
  reason: string;
  trustedFields: readonly { label: string; value: string }[];
}

export interface ActivityItem {
  id: string;
  label: string;
  detail: string;
  status: "complete" | "active" | "waiting" | "warning";
}

export interface ProviderConfiguration {
  kind: ProviderKind;
  baseUrl?: string;
  project?: string;
  authHeader?: string;
}

export interface ProviderProfile {
  id: string;
  providerId: string;
  displayName: string;
  configuration: ProviderConfiguration;
  scope: ProviderProfileScope;
  active: boolean;
  status: ProviderStatus;
  safeError?: string;
  credentialSource: CredentialSource;
  credentialState: ProviderCredentialState;
  defaultModelId?: ModelId;
  defaultReasoningEffort?: ReasoningEffort;
}

export interface ProviderProfileInput {
  id: string;
  configuration: ProviderConfiguration;
}

export interface ClientSnapshot {
  schemaVersion: typeof CLIENT_SCHEMA_VERSION;
  transportRevision: Revision;
  runtimeMode: "native" | "fixture";
  connection: ConnectionState;
  connectionId?: string;
  activeSessionId?: SessionId;
  sessions: readonly SessionSummary[];
  catalog: CatalogProjection;
  providers: readonly ProviderProfile[];
  settings: ClientSettingsProjection;
  providerRecoveryPending: string;
  activeSession?: ActiveSessionProjection;
  pendingPermission?: PermissionRequest;
  activity: readonly ActivityItem[];
}

export type ClientCommand =
  | { type: "create_session" }
  | { type: "open_session"; sessionId: SessionId }
  | { type: "rename_session"; sessionId: SessionId; title: string }
  | { type: "archive_session"; sessionId: SessionId }
  | { type: "unarchive_session"; sessionId: SessionId }
  | { type: "export_transcript"; sessionId: SessionId }
  | { type: "delete_session"; sessionId: SessionId }
  | { type: "upsert_provider_profile"; profile: ProviderProfileInput }
  | { type: "duplicate_provider_profile"; sourceId: string; destinationId: string }
  | { type: "activate_provider_profile"; connectionId: string }
  | { type: "test_provider_profile"; connectionId: string }
  | {
      type: "set_provider_defaults";
      connectionId: string;
      modelId: ModelId;
      reasoningEffort?: ReasoningEffort;
    }
  | { type: "disconnect_provider_profile"; connectionId: string }
  | { type: "delete_provider_profile"; connectionId: string }
  | { type: "update_client_preference"; change: ClientPreferenceChange }
  | { type: "start_codex_authentication" }
  | { type: "cancel_codex_authentication"; authenticationRequestId: RequestId }
  | { type: "refresh_catalog" }
  | { type: "select_model"; sessionId: SessionId; modelId: ModelId }
  | { type: "submit_prompt"; sessionId: SessionId; prompt: string }
  | { type: "cancel_attempt"; sessionId: SessionId; attemptId: AttemptId }
  | { type: "retry_attempt"; sessionId: SessionId; attemptId: AttemptId }
  | {
      type: "answer_permission";
      sessionId: SessionId;
      toolCallId: string;
      decision: "allow_once" | "deny";
    };

export interface CommandReceipt {
  requestId: RequestId;
}

export type NoticeLevel = "info" | "success" | "warning" | "error";

export type ClientFrame =
  | {
      kind: "snapshot";
      reason: "initial" | "projection" | "resynchronization";
      revision: Revision;
      snapshot: ClientSnapshot;
    }
  | {
      kind: "active_session_delta";
      revision: Revision;
      sessionId: SessionId;
      sessionRevision: Revision;
      summary: SessionSummary;
      selectedModelId?: ModelId;
      transcript: {
        start: number;
        deleteCount: number;
        items: readonly TranscriptItem[];
      };
      attempt?: AttemptProjection;
      pendingPermission?: PermissionRequest;
    }
  | {
      kind: "notice";
      revision: Revision;
      requestId?: RequestId;
      level: NoticeLevel;
      code: string;
      message: string;
    };

export interface CredentialSubmission {
  connectionId: string;
  operation: "session_only" | "save" | "replace";
  credential: string;
}

export interface ClientTransport {
  connect(onFrame: (frame: ClientFrame) => void, onError: (error: unknown) => void): Promise<ClientSnapshot>;
  command(command: ClientCommand): Promise<CommandReceipt>;
  snapshot(lastAppliedRevision?: Revision): Promise<ClientSnapshot>;
  submitCredential(secret: CredentialSubmission): Promise<CommandReceipt>;
  close(): Promise<void>;
}

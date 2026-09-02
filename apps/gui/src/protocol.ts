export const CLIENT_SCHEMA_VERSION = 1 as const;
export const MAX_PROMPT_UTF8_BYTES = 128 * 1024;
export const MAX_SESSION_TITLE_UTF8_BYTES = 128;

export type SessionId = string;
export type AttemptId = string;
export type ModelId = string;
export type RequestId = string;
export type Revision = string;
export type CommandOutcome = "committed" | "rejected" | "unknown";

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

export interface ClientSnapshot {
  schemaVersion: typeof CLIENT_SCHEMA_VERSION;
  transportRevision: Revision;
  runtimeMode: "native" | "fixture";
  connection: ConnectionState;
  connectionId?: string;
  activeSessionId?: SessionId;
  sessions: readonly SessionSummary[];
  catalog: CatalogProjection;
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
      kind: "notice";
      revision: Revision;
      requestId?: RequestId;
      level: NoticeLevel;
      code: string;
      message: string;
    };

export interface EphemeralCredential {
  connectionId: string;
  credential: string;
}

export interface ClientTransport {
  connect(onFrame: (frame: ClientFrame) => void, onError: (error: unknown) => void): Promise<ClientSnapshot>;
  command(command: ClientCommand): Promise<CommandReceipt>;
  snapshot(lastAppliedRevision?: Revision): Promise<ClientSnapshot>;
  submitCredential(secret: EphemeralCredential): Promise<CommandReceipt>;
  close(): Promise<void>;
}

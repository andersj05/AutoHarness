// Renderer-neutral memory data. Text is always inert.
export type MemoryStatus = "active" | "proposed" | "conflicting" | "superseded" | "rejected" | "retracted" | "expired" | "deleted";
export type MemoryScope = "user" | "workspace" | "session" | "agent";
export type MemoryTrust = "user_approved" | "verified_observation" | "imported" | "untrusted_proposal";
export type MemoryOrigin = "explicit_user" | "verified_tool" | "imported_document" | "model_proposal" | "compaction";
export type MemorySensitivity = "public" | "internal" | "sensitive" | "secret";
export type MemoryEvidenceAvailability = "retained" | "absent" | "erased";
export type MemoryRelationKind = "duplicate_of" | "contradicts" | "refines" | "supersedes" | "related" | "derived_from";
export type MemoryFindingKind = "duplicate" | "contradiction" | "secret_detected" | "unsupported_scope" | "malformed_content" | "policy_conflict" | "injection_pattern" | "ungrounded_evidence";
export type MemoryStatusFilter = "eligible" | "all" | "active" | "proposed" | "inactive";
export type MemoryScopeFilter = "all" | "user" | "workspace" | "session" | "agent";
export type MemoryPageDirection = "first" | "next" | "previous";
export interface MemoryQuery {
  view_generation: string;
  literal: string;
  status: MemoryStatusFilter;
  scope: MemoryScopeFilter;
  direction: MemoryPageDirection;
  before: string | null;
}
export interface MemoryEvidence {
  label: string;
  source: string;
  excerpt: string | null;
  availability: MemoryEvidenceAvailability;
}
export interface MemoryRelation {
  kind: MemoryRelationKind;
  memory_id: string;
}
export interface MemoryFinding {
  kind: MemoryFindingKind;
  related_memory_id: string;
  summary: string;
}
export interface MemoryRevisionContext {
  expected_last_sequence: string;
  revision_id: string;
  proposal_revision_id: string | null;
  scope_identity: string;
  origin: MemoryOrigin;
  sensitivity: MemorySensitivity;
  evidence: readonly (MemoryEvidence)[];
  relations: readonly (MemoryRelation)[];
  findings: readonly (MemoryFinding)[];
}
export interface MemoryAdmissionContext {
  provider_attempt: string;
  run_turn: number;
  epoch: string;
  token_count: number;
  source_revision: string;
  renderer_version: string;
  reason_factors: readonly (string)[];
}
export interface MemoryAdmission {
  session: string;
  model: string;
  reason: string;
  admitted_at_ms: string;
  rank: number;
  context: MemoryAdmissionContext | null;
}
export interface MemoryDetail {
  revision: number;
  content: string | null;
  source: string;
  trust: MemoryTrust;
  created_at_ms: string;
  valid_until_ms: string | null;
  admissions: readonly (MemoryAdmission)[];
  revision_context: MemoryRevisionContext | null;
}
export interface MemoryRow {
  memory_id: string;
  preview: string;
  status: MemoryStatus;
  scope: MemoryScope;
  updated_at_ms: string;
  confidence_bps: number | null;
  admission_count: number;
  detail: MemoryDetail | null;
}
export interface MemoryProjection {
  view_generation: string;
  generation: string;
  state: MemoryLoadState;
  rows: readonly (MemoryRow)[];
  total: number;
  stale: boolean;
  next_cursor: string | null;
}

export type MemoryLoadState = { kind: "ready" | "loading" } | { kind: "failed"; payload: { failure: { message: string } } };
export type MemoryCommand =
  | { kind: "query"; payload: MemoryQuery }
  | { kind: "remember"; payload: { content: string } }
  | { kind: "import"; payload: { path: string } }
  | { kind: "revise"; payload: { memory_id: string; expected_last_sequence: string; content: string } }
  | { kind: "approve" | "reject"; payload: { memory_id: string; expected_last_sequence: string; proposal_revision_id: string } }
  | { kind: "retract"; payload: { memory_id: string; expected_last_sequence: string; revision_id: string } }
  | { kind: "delete"; payload: { memory_id: string; expected_last_sequence: string } }
  | { kind: "export"; payload: { memory_id: string } };

export function emptyMemory(): MemoryProjection {
  return { view_generation: "0", generation: "0", state: { kind: "ready" }, rows: [], total: 0, stale: false, next_cursor: null };
}

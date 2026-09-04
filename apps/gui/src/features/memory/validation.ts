import type { MemoryProjection } from "./model";

const encoder = new TextEncoder();
const u64 = (value: string) => typeof value === "string" && /^(0|[1-9]\d*)$/.test(value) && BigInt(value) <= 18_446_744_073_709_551_615n;

/** Reject malformed or over-budget data before any component receives it. */
export function validateMemory(page: MemoryProjection): void {
  const invalid = () => { throw new Error("The host published an invalid memory page"); };
  if (!page || !u64(page.view_generation) || !u64(page.generation) || !Array.isArray(page.rows) || page.rows.length > 100
    || !Number.isInteger(page.total) || page.total < page.rows.length || page.total > 4_294_967_295
    || typeof page.stale !== "boolean" || !["ready", "loading", "failed"].includes(page.state?.kind)) invalid();
  let bytes = 0;
  const visit = (value: unknown, depth = 0): void => {
    if (depth > 12) invalid();
    if (typeof value === "string") {
      const size = encoder.encode(value).length;
      if (size > 65_536) invalid();
      bytes += size;
      if (bytes > 8 * 1024 * 1024) invalid();
    } else if (Array.isArray(value)) {
      if (value.length > 352) invalid();
      value.forEach((entry) => visit(entry, depth + 1));
    } else if (value && typeof value === "object") Object.values(value).forEach((entry) => visit(entry, depth + 1));
  };
  visit(page);
  const ids = new Set<string>();
  for (const row of page.rows) {
    if (typeof row.memory_id !== "string" || !row.memory_id || ids.has(row.memory_id) || typeof row.preview !== "string"
      || !["active", "proposed", "conflicting", "superseded", "rejected", "retracted", "expired", "deleted"].includes(row.status)
      || !["user", "workspace", "session", "agent"].includes(row.scope)) invalid();
    ids.add(row.memory_id);
    const detail = row.detail;
    if (detail) {
      if (!Array.isArray(detail.admissions) || detail.admissions.length > 64 || !["user_approved", "verified_observation", "imported", "untrusted_proposal"].includes(detail.trust)) invalid();
      const context = detail.revision_context;
      if (context && (!u64(context.expected_last_sequence) || !context.revision_id || !Array.isArray(context.evidence)
        || context.evidence.length > 64 || !Array.isArray(context.relations) || context.relations.length > 64 || !Array.isArray(context.findings))) invalid();
    }
  }
}

import { emptyMemory, type MemoryCommand, type MemoryProjection, type MemoryQuery, type MemoryRow } from "./model";

export function fixtureMemoryRow(index: number, proposed = false): MemoryRow {
  const identity = `memory-${index}`;
  const content = proposed ? "Prefer small, independently verifiable changes." : "Keep durable runtime decisions in Rust and presentation state in the client.";
  return {
    memory_id: identity, preview: content, status: proposed ? "proposed" : "active", scope: "workspace",
    updated_at_ms: "1788433200000", confidence_bps: proposed ? 8200 : null, admission_count: proposed ? 0 : 1,
    detail: {
      revision: 1, content, source: proposed ? "Imported document: sha256:fixture-document" : "Explicit user instruction",
      trust: proposed ? "imported" : "user_approved", created_at_ms: "1788430000000", valid_until_ms: null,
      revision_context: {
        expected_last_sequence: "3", revision_id: `${identity}-revision-1`, proposal_revision_id: proposed ? `${identity}-revision-1` : null,
        scope_identity: "workspace-fixture", origin: proposed ? "imported_document" : "explicit_user", sensitivity: "internal",
        evidence: [{ label: "Source excerpt", source: "document:sha256:fixture", excerpt: content, availability: "retained" }],
        relations: proposed ? [{ kind: "related", memory_id: "memory-1" }] : [], findings: [],
      },
      admissions: proposed ? [] : [{ session: "session-fixture", model: "Fixture model", reason: "Eligible workspace constraint",
        admitted_at_ms: "1788433200000", rank: 1,
        context: { provider_attempt: "attempt-fixture", run_turn: 1, epoch: "epoch-fixture", token_count: 24,
          source_revision: `${identity}-revision-1`, renderer_version: "memory-v1", reason_factors: ["scope match", "user approved"] } }],
    },
  };
}

/** Simulated host used only by browser fixtures. Production authority stays in Rust. */
export class FixtureMemory {
  private rows = [fixtureMemoryRow(1), fixtureMemoryRow(2, true)];
  private generation = 1n;
  private query: MemoryQuery = { view_generation: "0", literal: "", scope: "all", status: "all", direction: "first", before: null };

  page(): MemoryProjection {
    const needle = this.query.literal.toLocaleLowerCase();
    const filtered = this.rows.filter((row) => {
      const status = this.query.status;
      return (this.query.scope === "all" || row.scope === this.query.scope)
        && (status === "all" || ((status === "eligible" || status === "active") && row.status === "active")
          || (status === "proposed" && row.status === "proposed")
          || (status === "inactive" && !["active", "proposed"].includes(row.status)))
        && (!needle || `${row.memory_id} ${row.detail?.content ?? ""}`.toLocaleLowerCase().includes(needle));
    });
    const start = this.query.before ? Number(this.query.before) : 0;
    const rows = filtered.slice(start, start + 100);
    return { ...emptyMemory(), view_generation: this.query.view_generation, generation: this.generation.toString(), rows,
      total: rows.length, next_cursor: start + 100 < filtered.length ? String(start + 100) : null };
  }

  command(command: MemoryCommand): MemoryProjection {
    if (command.kind === "query") { this.query = command.payload; return this.page(); }
    if (command.kind === "remember" || command.kind === "import") {
      const row = fixtureMemoryRow(this.rows.length + 1, command.kind === "import");
      if (command.kind === "remember" && row.detail) row.detail.content = row.preview = command.payload.content;
      this.rows.unshift(row);
    } else if (command.kind !== "export") {
      const row = this.rows.find((entry) => entry.memory_id === command.payload.memory_id);
      const detail = row?.detail;
      const context = detail?.revision_context;
      if (!row || !detail || !context || context.expected_last_sequence !== command.payload.expected_last_sequence) {
        throw new Error("Memory changed. Refresh and review the latest revision.");
      }
      if (command.kind === "approve" || command.kind === "reject") {
        if (context.proposal_revision_id !== command.payload.proposal_revision_id) throw new Error("Proposal changed.");
        row.status = command.kind === "approve" ? "active" : "rejected";
        if (command.kind === "approve") {
          detail.trust = "user_approved";
          detail.revision += 1;
          context.revision_id = `${row.memory_id}-revision-${detail.revision}`;
        }
        context.proposal_revision_id = null;
      } else if (command.kind === "revise") {
        row.preview = detail.content = command.payload.content;
        detail.revision += 1;
        context.revision_id = `${row.memory_id}-revision-${detail.revision}`;
      } else if (command.kind === "retract") {
        if (command.payload.revision_id !== context.revision_id) throw new Error("Revision changed.");
        row.status = "retracted";
      } else if (command.kind === "delete") {
        row.status = "deleted"; row.preview = "Content erased"; detail.content = null;
        context.evidence = context.evidence.map((evidence) => ({ ...evidence, excerpt: null, availability: "erased" }));
      }
      context.expected_last_sequence = String(BigInt(context.expected_last_sequence) + 3n);
    }
    this.generation += 1n;
    return this.page();
  }
}

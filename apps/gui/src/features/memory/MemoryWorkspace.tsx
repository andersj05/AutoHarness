import { useEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
import type { ClientCommand, CommandOutcome } from "../../protocol";
import { Icon } from "../../components/Icon";
import { Button, Chip, Dialog, Field } from "../../components/primitives";
import { securityDisplaySafe as safe } from "../../securityText";
import { InertText, SafeDiff } from "../../components/primitives/Content";
import type { MemoryCommand, MemoryProjection, MemoryQuery, MemoryRow } from "./model";

interface Props {
  memory: MemoryProjection;
  blocked: boolean;
  sessionId?: string;
  onCommand: (command: ClientCommand) => Promise<CommandOutcome>;
  onOpenNavigation: () => void;
  onDialogChange: (open: boolean) => void;
}

type Action = "remember" | "import" | "revise" | "approve" | "reject" | "retract" | "delete" | "export";
type Review = { action: Action; row?: MemoryRow };
const label = (value: string) => value.replaceAll("_", " ");
const titles: Record<Action, string> = { remember: "Remember an instruction", import: "Import for review", revise: "Correct memory", approve: "Approve proposal", reject: "Reject proposal", retract: "Retract memory", delete: "Delete memory content", export: "Export memory" };

function date(value: string | null): string {
  if (value === null) return "No expiry";
  const numeric = Number(value);
  return Number.isSafeInteger(numeric) && Math.abs(numeric) <= 8_640_000_000_000_000 ? new Date(numeric).toISOString().replace("T", " ").replace(".000Z", " UTC") : value;
}

export function MemoryWorkspace({ memory, blocked, sessionId, onCommand, onOpenNavigation, onDialogChange }: Props) {
  const [query, setQuery] = useState<Omit<MemoryQuery, "view_generation">>({ literal: "", status: "all", scope: "all", direction: "first", before: null });
  const [search, setSearch] = useState("");
  const [history, setHistory] = useState<(string | null)[]>([]);
  const [selectedId, setSelectedId] = useState<string>();
  const [requested, setRequested] = useState<string>();
  const [queryFailed, setQueryFailed] = useState(false);
  const [refresh, setRefresh] = useState(0);
  const [review, setReview] = useState<Review>();
  const [draft, setDraft] = useState("");
  const [confirmation, setConfirmation] = useState("");
  const [busy, setBusy] = useState(false);
  const [message, setMessage] = useState("");
  const generation = useRef(BigInt(memory.view_generation));
  const callbacks = useRef({ onCommand, onDialogChange });
  callbacks.current = { onCommand, onDialogChange };
  const selected = memory.rows.find((row) => row.memory_id === selectedId) ?? memory.rows[0];
  const ready = !blocked && !queryFailed && memory.state.kind === "ready" && !memory.stale && requested === memory.view_generation;

  useEffect(() => {
    if (blocked) return;
    generation.current = (generation.current > BigInt(memory.view_generation) ? generation.current : BigInt(memory.view_generation)) + 1n;
    const view_generation = generation.current.toString();
    setRequested(view_generation);
    setQueryFailed(false);
    let current = true;
    void callbacks.current.onCommand({ type: "memory", command: { kind: "query", payload: { ...query, view_generation } } }).then((outcome) => {
      if (current && outcome !== "committed") setQueryFailed(true);
    });
    return () => { current = false; };
    // The authoritative response must not recursively issue another query.
  }, [query, refresh, sessionId, blocked]);

  useEffect(() => {
    if (blocked) { setReview(undefined); setDraft(""); setConfirmation(""); }
  }, [blocked]);
  useEffect(() => {
    callbacks.current.onDialogChange(Boolean(review));
    return () => callbacks.current.onDialogChange(false);
  }, [review]);

  const changeQuery = (next: Partial<typeof query>) => {
    setHistory([]);
    setSelectedId(undefined);
    setQuery((current) => ({ ...current, ...next, before: null, direction: "first" }));
  };
  const open = (action: Action) => {
    setReview({ action, row: selected ? structuredClone(selected) : undefined });
    setDraft(action === "revise" ? selected?.detail?.content ?? "" : "");
    setConfirmation(""); setMessage("");
  };
  const target = review?.row;
  const context = target?.detail?.revision_context;
  const latest = target ? memory.rows.find((row) => row.memory_id === target.memory_id) : undefined;
  const needsTarget = review && !["remember", "import"].includes(review.action);
  const staleTarget = Boolean(needsTarget && (!latest || latest.detail?.revision_context?.expected_last_sequence !== context?.expected_last_sequence));
  const invalidDraft = review && ["remember", "revise", "import"].includes(review.action)
    && (!draft.trim() || new TextEncoder().encode(draft).length > (review.action === "import" ? 4096 : 16384));

  const submit = async () => {
    if (!review || busy || !ready || staleTarget || invalidDraft) return;
    const { action } = review;
    let command: MemoryCommand;
    if (action === "remember") command = { kind: "remember", payload: { content: draft } };
    else if (action === "import") command = { kind: "import", payload: { path: draft } };
    else if (action === "export" && target) command = { kind: "export", payload: { memory_id: target.memory_id } };
    else {
      if (!target || !context) return;
      const identity = { memory_id: target.memory_id, expected_last_sequence: context.expected_last_sequence };
      if (action === "revise") command = { kind: "revise", payload: { ...identity, content: draft } };
      else if (action === "approve" || action === "reject") {
        if (!context.proposal_revision_id) return;
        command = { kind: action, payload: { ...identity, proposal_revision_id: context.proposal_revision_id } };
      } else if (action === "retract") command = { kind: "retract", payload: { ...identity, revision_id: context.revision_id } };
      else if (action === "delete") {
        if (confirmation !== target.memory_id) return;
        command = { kind: "delete", payload: identity };
      } else command = { kind: "export", payload: { memory_id: target.memory_id } };
    }
    setBusy(true);
    const outcome = await callbacks.current.onCommand({ type: "memory", command });
    setBusy(false);
    if (outcome === "committed") {
      setReview(undefined); setDraft("");
      setMessage(action === "export" ? "Export committed. The JSON file is beside the database." : action === "import" ? "Imported as an untrusted proposal. Review it before approval." : "The host committed the memory change.");
      setRefresh((value) => value + 1);
    } else setMessage(outcome === "unknown" ? "Outcome unknown. Refresh and inspect the ledger before trying again." : "The host rejected this change. Refresh and review its current state.");
  };

  const detail = selected?.detail;
  const revision = detail?.revision_context;
  const hasContent = detail?.content !== null && detail?.content !== undefined;
  const reviewable = hasContent && revision?.proposal_revision_id && ["proposed", "conflicting"].includes(selected?.status ?? "");
  const correctable = hasContent && revision && (selected?.status === "active" || selected?.status === "expired" || (selected?.status === "conflicting" && !revision.proposal_revision_id));
  const retractable = revision && (correctable || selected?.status === "active");
  return <main className="routeWorkspace memoryWorkspace" id="main-content" tabIndex={-1} aria-label="Memory workspace">
    <header className="routeWorkspaceHeader memoryHeader">
      <button aria-label="Open navigation" className="iconButton mobileMenu" onClick={onOpenNavigation} type="button"><Icon name="menu" /></button>
      <div><p className="eyebrow">Knowledge ledger</p><h1>Memory</h1><p>Inspect what is remembered, where it came from, and when it was used.</p></div>
      <div className="memoryActions"><Button disabled={!ready || busy} onClick={() => open("import")}>Import document</Button><Button disabled={!ready || busy} onClick={() => open("remember")} variant="primary">Remember</Button></div>
    </header>
    <form className="memoryFilters" onSubmit={(event) => { event.preventDefault(); changeQuery({ literal: search }); }}>
      <Field label="Search memory" maxLength={256} onChange={(event) => setSearch(event.target.value)} placeholder="Search literal memory text" type="search" value={search} />
      <label>Status<select aria-label="Memory status" value={query.status} onChange={(event) => changeQuery({ status: event.target.value as typeof query.status })}>{["all", "eligible", "active", "proposed", "inactive"].map((value) => <option key={value} value={value}>{label(value)}</option>)}</select></label>
      <label>Scope<select aria-label="Memory scope" value={query.scope} onChange={(event) => changeQuery({ scope: event.target.value as typeof query.scope })}>{["all", "user", "workspace", "session", "agent"].map((value) => <option key={value} value={value}>{value}</option>)}</select></label>
      <Button disabled={blocked || busy} type="submit" icon="search">Search</Button><Button disabled={blocked || busy} onClick={() => setRefresh((value) => value + 1)} icon="refresh">Refresh</Button>
    </form>
    <p className="memoryStatus" role="status">{queryFailed ? "Memory query failed. Refresh to retry." : memory.state.kind === "failed" ? memory.state.payload.failure.message : !ready ? "Loading authoritative memory..." : `${memory.rows.length} records on this page · Ledger generation ${memory.generation}`}</p>
    <div className="memoryColumns" aria-busy={!ready}>
      <section className="memoryList" aria-label="Memory records">
        {ready && memory.rows.length === 0 ? <div className="memoryEmpty"><Icon name="memory" size={28} /><h2>No matching memory</h2><p>Try another filter, remember an instruction, or import a document for review.</p></div> : null}
        {ready ? memory.rows.map((row) => <button className="memoryRow" aria-pressed={selected?.memory_id === row.memory_id} key={row.memory_id} onClick={() => setSelectedId(row.memory_id)} type="button">
          <span className="memoryRowMeta"><span data-status={row.status}>{row.status}</span><span>{row.scope}</span></span>
          <strong>{safe(row.preview)}</strong><small>{safe(row.memory_id)}</small>
          <span className="memoryRowFooter">{row.admission_count} admissions · {date(row.updated_at_ms).slice(0, 10)}</span>
        </button>) : null}
        <nav className="memoryPaging" aria-label="Memory pages">
          <Button disabled={!ready || history.length === 0 || busy} onClick={() => { const previous = history.at(-1) ?? null; setHistory((entries) => entries.slice(0, -1)); setQuery((current) => ({ ...current, direction: "previous", before: previous })); setSelectedId(undefined); }}>Previous</Button>
          <span>Page {history.length + 1}</span>
          <Button disabled={!ready || !memory.next_cursor || busy} onClick={() => { setHistory((entries) => [...entries, query.before]); setQuery((current) => ({ ...current, direction: "next", before: memory.next_cursor })); setSelectedId(undefined); }}>Next</Button>
        </nav>
      </section>
      <section className="memoryDetail" aria-label="Memory detail">
        {ready && selected && detail ? <>
          <div className="memoryDetailHeading"><div><p className="eyebrow">{selected.scope} memory · Revision {detail.revision}</p><h2>{selected.status === "proposed" ? "Proposal review" : "Remembered context"}</h2></div><Chip intent={selected.status === "active" ? "success" : "warning"}>{selected.status}</Chip></div>
          {(detail.trust !== "user_approved" || selected.status === "proposed") && <div className="memoryWarning"><strong>Untrusted source</strong><p>{selected.status === "proposed" ? "This proposal cannot authorize itself. Approval commits a distinct user-approved revision before future admission." : "Source provenance does not grant authority. Eligibility remains subject to host validation."}</p></div>}
          <InertText text={detail.content ?? "Content erased. Only audit metadata remains."} />
          <div className="memoryActions">
            {reviewable ? <><Button disabled={busy} onClick={() => open("approve")} variant="primary">Review approval</Button><Button disabled={busy} onClick={() => open("reject")}>Reject</Button></> : null}
            {correctable ? <Button disabled={busy} onClick={() => open("revise")}>Correct</Button> : null}
            {retractable ? <Button disabled={busy} onClick={() => open("retract")}>Retract</Button> : null}
            <Button disabled={busy} onClick={() => open("export")}>Export</Button>
            {selected.status !== "deleted" ? <Button disabled={busy || !revision} onClick={() => open("delete")} variant="quiet" className="dangerText">Delete content</Button> : null}
          </div>
          <dl className="memoryFacts"><div><dt>Identity</dt><dd>{safe(selected.memory_id)}</dd></div><div><dt>Trust</dt><dd>{label(detail.trust)}</dd></div><div><dt>Source</dt><dd>{safe(detail.source)}</dd></div><div><dt>Scope identity</dt><dd>{safe(revision?.scope_identity ?? "Unavailable")}</dd></div><div><dt>Sensitivity</dt><dd>{revision?.sensitivity ?? "Unavailable"}</dd></div><div><dt>Expires</dt><dd>{date(detail.valid_until_ms)}</dd></div><div><dt>Confidence</dt><dd>{selected.confidence_bps === null ? "Not reported" : `${selected.confidence_bps / 100}% (not authority)`}</dd></div></dl>
          <section aria-label="Provenance timeline"><h3>Provenance timeline</h3><ol className="memoryTimeline"><li><strong>{label(revision?.origin ?? "source unavailable")}</strong><span>{date(detail.created_at_ms)}</span></li><li><strong>Current revision {detail.revision} · {selected.status}</strong><code>{safe(revision?.revision_id ?? "Unavailable")}</code><span>{date(selected.updated_at_ms)}</span></li>{revision?.proposal_revision_id && <li><strong>Awaiting distinct approval revision</strong><code>{safe(revision.proposal_revision_id)}</code></li>}</ol></section>
          <section aria-label="Memory evidence"><h3>Evidence</h3>{revision?.evidence.length ? revision.evidence.map((evidence, index) => <details className="memoryEvidence" key={index}><summary>{safe(evidence.label)} <small>{evidence.availability}</small></summary><p>{safe(evidence.source)}</p><InertText text={evidence.excerpt ?? (evidence.availability === "erased" ? "Excerpt erased by logical deletion." : "No excerpt was recorded.")} /></details>) : <p className="muted">No retained evidence.</p>}</section>
          <section aria-label="Memory relations"><h3>Relations and validation</h3>{revision?.relations.map((relation, index) => <div className="memoryRelation" key={index}><span>{label(relation.kind)}</span><code>{safe(relation.memory_id)}</code>{memory.rows.some((row) => row.memory_id === relation.memory_id) ? <Button size="small" onClick={() => setSelectedId(relation.memory_id)}>Inspect</Button> : <small>Outside this page</small>}</div>)}{revision?.findings.map((finding, index) => <div className="memoryWarning" key={index}><strong>{label(finding.kind)}</strong><p>{safe(finding.summary)}</p><code>{safe(finding.related_memory_id)}</code></div>)}{!revision?.relations.length && !revision?.findings.length ? <p className="muted">No relations or validation findings.</p> : null}</section>
          <section aria-label="Admission history"><h3>Admission history <small>{detail.admissions.length} shown / {selected.admission_count} recorded</small></h3>{detail.admissions.length === 0 ? <p className="muted">This revision has not been admitted into a provider turn.</p> : detail.admissions.map((admission, index) => <details className="memoryEvidence" key={index}><summary>{safe(admission.model)} <small>{date(admission.admitted_at_ms)}</small></summary><p>{safe(admission.reason)}</p><dl className="memoryFacts"><div><dt>Session</dt><dd>{safe(admission.session)}</dd></div><div><dt>Rank</dt><dd>{admission.rank}</dd></div>{admission.context && <><div><dt>Attempt / turn</dt><dd>{safe(admission.context.provider_attempt)} / {admission.context.run_turn}</dd></div><div><dt>Epoch</dt><dd>{safe(admission.context.epoch)}</dd></div><div><dt>Source revision</dt><dd>{safe(admission.context.source_revision)}</dd></div><div><dt>Renderer / tokens</dt><dd>{safe(admission.context.renderer_version)} / {admission.context.token_count}</dd></div><div><dt>Factors</dt><dd>{admission.context.reason_factors.map(safe).join(", ")}</dd></div></>}</dl></details>)}</section>
        </> : <div className="memoryEmpty"><Icon name="memory" size={28} /><h2>{ready ? "Select a memory" : "Refreshing the ledger"}</h2><p>Revision details, provenance, evidence, and admissions appear here.</p></div>}
      </section>
    </div>
    <p role="status" className="memoryStatus">{message}</p>
    {review && !blocked ? createPortal(<Dialog title={titles[review.action]} eyebrow="Exact memory scope" onClose={() => { if (!busy) setReview(undefined); }} description={review.action === "approve" ? "You are approving the exact proposed revision below. The host will validate and commit a new user-approved revision." : review.action === "delete" ? "Erase retained memory content and evidence excerpts. Audit identities remain. Existing user-owned exports are not removed. Export first if you need a copy." : review.action === "retract" ? "Prevent this exact revision from admission into future turns. Existing admission history remains auditable." : review.action === "import" ? "Enter a workspace-relative UTF-8 text file, at most 16 KiB. The Rust host reads the file and creates an untrusted proposal for separate review." : review.action === "export" ? "Write a standalone JSON export beside the database. It contains retained memory content and audit history." : "The Rust host validates and persists this deliberate change."}
      footer={<><Button disabled={busy} onClick={() => setReview(undefined)} variant="quiet">Cancel</Button><Button disabled={!ready || staleTarget || Boolean(invalidDraft) || (review.action === "delete" && confirmation !== target?.memory_id)} loading={busy} onClick={() => void submit()} variant={review.action === "delete" ? "danger" : "primary"}>{titles[review.action]}</Button></>}>
      {target && needsTarget ? <div className="memoryReviewScope"><strong>{safe(target.memory_id)}</strong><span>Revision {target.detail?.revision} · Sequence {context?.expected_last_sequence}</span><code>{safe(context?.proposal_revision_id ?? context?.revision_id ?? "")}</code></div> : null}
      {staleTarget ? <p role="alert">This memory changed while you were reviewing it. Close this dialog and review the latest revision.</p> : null}
      {["remember", "revise"].includes(review.action) ? <label className="memoryDraft">Memory content<textarea autoComplete="off" data-initial-focus maxLength={16384} onChange={(event) => setDraft(event.target.value)} rows={6} value={draft} /></label> : null}
      {review.action === "import" ? <Field label="Workspace-relative document path" autoComplete="off" data-initial-focus maxLength={1024} onChange={(event) => setDraft(event.target.value)} value={draft} placeholder="docs/project-notes.md" /> : null}
      {review.action === "revise" ? <SafeDiff before={target?.detail?.content ?? ""} after={draft} /> : needsTarget ? <InertText text={target?.detail?.content ?? "Content erased"} /> : null}
      {review.action === "approve" && <><p className="memoryWarning">Origin: {label(context?.origin ?? "unknown")} · Trust: {label(target?.detail?.trust ?? "unknown")}. Approval does not change the original provenance.</p>{context?.findings.map((finding, index) => <p key={index}>{label(finding.kind)}: {safe(finding.summary)}</p>)}</>}
      {review.action === "delete" ? <Field label="Confirm memory identity" hint="Type the complete identity above to confirm deletion." autoComplete="off" data-initial-focus onChange={(event) => setConfirmation(event.target.value)} value={confirmation} /> : null}
      {invalidDraft && draft ? <p role="alert">Use non-empty content within the documented byte limit.</p> : null}
      <p role="status">{message}</p>
    </Dialog>, document.querySelector(".app") ?? document.body) : null}
  </main>;
}

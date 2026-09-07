import { useEffect, useMemo, useState } from "react";
import { createPortal } from "react-dom";
import {
  MAX_SESSION_TITLE_UTF8_BYTES,
  type ClientCommand,
  type ClientSnapshot,
  type CommandOutcome,
  type SessionSummary,
  type TimestampStyle,
} from "../protocol";
import { Icon } from "./Icon";
import { Button, Dialog, Field, VirtualList } from "./primitives";

type SessionFilter = "open" | "archived" | "all";
type SessionDialog =
  | { kind: "rename"; session: SessionSummary }
  | { kind: "archive"; session: SessionSummary }
  | { kind: "delete"; session: SessionSummary };

interface SessionsWorkspaceProps {
  snapshot: ClientSnapshot;
  onCreate: () => void;
  onDialogChange?: (open: boolean) => void;
  onCommand: (command: ClientCommand) => Promise<CommandOutcome>;
  onOpen: (id: string) => void;
  onOpenNavigation: () => void;
  timestampStyle: TimestampStyle;
}

function titleError(title: string): string | undefined {
  if (!title.trim()) return "Enter a visible session title.";
  if ([...title].some((character) => /[\u0000-\u001f\u007f]/.test(character))) {
    return "Session titles cannot contain control characters.";
  }
  if (new TextEncoder().encode(title).length > MAX_SESSION_TITLE_UTF8_BYTES) {
    return `Keep the title within ${MAX_SESSION_TITLE_UTF8_BYTES} UTF-8 bytes.`;
  }
  return undefined;
}

function formattedDate(value: string | undefined, style: TimestampStyle): string {
  if (style === "hidden") return "Hidden by preference";
  if (!value) return "Update time unavailable";
  const date = new Date(value);
  if (!Number.isFinite(date.getTime())) return "Update time unavailable";
  if (style === "relative") {
    const deltaSeconds = Math.round((date.getTime() - Date.now()) / 1000);
    const absoluteSeconds = Math.abs(deltaSeconds);
    const [relativeValue, unit] = absoluteSeconds < 3_600
      ? [Math.round(deltaSeconds / 60), "minute" as const]
      : absoluteSeconds < 86_400
        ? [Math.round(deltaSeconds / 3_600), "hour" as const]
        : [Math.round(deltaSeconds / 86_400), "day" as const];
    return new Intl.RelativeTimeFormat(undefined, { numeric: "auto" }).format(relativeValue, unit);
  }
  return new Intl.DateTimeFormat(undefined, { dateStyle: "medium", timeStyle: "short" }).format(date);
}

export function SessionsWorkspace({ snapshot, onCommand, onCreate, onDialogChange, onOpen, onOpenNavigation, timestampStyle }: SessionsWorkspaceProps) {
  const [query, setQuery] = useState("");
  const [sort, setSort] = useState("recent");
  const [filter, setFilter] = useState<SessionFilter>("open");
  const [selectedId, setSelectedId] = useState(() => snapshot.activeSessionId ?? snapshot.sessions[0]?.id);
  const [dialog, setDialog] = useState<SessionDialog>();
  const [renameTitle, setRenameTitle] = useState("");
  const [deleteConfirmation, setDeleteConfirmation] = useState("");
  const [busyAction, setBusyAction] = useState<string>();
  const [actionMessage, setActionMessage] = useState<string>();
  const [listHeight, setListHeight] = useState(() => Math.max(220, Math.min(650, window.innerHeight - 255)));

  useEffect(() => {
    if (snapshot.pendingPermission) setDialog(undefined);
  }, [snapshot.pendingPermission]);

  useEffect(() => {
    onDialogChange?.(Boolean(dialog));
    return () => onDialogChange?.(false);
  }, [dialog, onDialogChange]);

  useEffect(() => {
    const resize = () => setListHeight(Math.max(220, Math.min(650, window.innerHeight - 255)));
    window.addEventListener("resize", resize);
    return () => window.removeEventListener("resize", resize);
  }, []);

  useEffect(() => {
    if (selectedId && snapshot.sessions.some((session) => session.id === selectedId)) return;
    setSelectedId(snapshot.activeSessionId ?? snapshot.sessions[0]?.id);
  }, [selectedId, snapshot.activeSessionId, snapshot.sessions]);

  useEffect(() => {
    if (!dialog || snapshot.sessions.some((session) => session.id === dialog.session.id)) return;
    setDialog(undefined);
    setDeleteConfirmation("");
  }, [dialog, snapshot.sessions]);

  const visibleSessions = useMemo(() => {
    const needle = query.trim().toLocaleLowerCase();
    return snapshot.sessions.filter((session) => {
      if (filter === "open" && session.archived) return false;
      if (filter === "archived" && !session.archived) return false;
      return !needle
        || session.title.toLocaleLowerCase().includes(needle)
        || session.id.toLocaleLowerCase().includes(needle);
    }).sort((left, right) => {
      if (sort === "title") return left.title.localeCompare(right.title) || left.id.localeCompare(right.id);
      const timestamp = (value?: string) => value && Number.isFinite(Date.parse(value)) ? Date.parse(value) : 0;
      return timestamp(right.updatedAt) - timestamp(left.updatedAt) || left.id.localeCompare(right.id);
    });
  }, [filter, query, snapshot.sessions, sort]);
  const selected = visibleSessions.find((session) => session.id === selectedId) ?? visibleSessions[0];
  const openCount = snapshot.sessions.filter((session) => !session.archived).length;
  const archivedCount = snapshot.sessions.length - openCount;
  const renameError = titleError(renameTitle);

  const run = async (key: string, command: ClientCommand, success: string, closeDialog = true) => {
    if (busyAction) return;
    setBusyAction(key);
    setActionMessage(undefined);
    const outcome = await onCommand(command);
    setBusyAction(undefined);
    if (outcome === "committed") {
      setActionMessage(success);
      if (closeDialog) {
        setDialog(undefined);
        setDeleteConfirmation("");
      }
    }
  };

  const openDialog = (next: SessionDialog) => {
    if (busyAction) return;
    setDialog(next);
    setRenameTitle(next.session.title);
    setDeleteConfirmation("");
  };

  return (
    <main className="routeWorkspace sessionRouteWorkspace" id="main-content" tabIndex={-1}>
      <header className="routeWorkspaceHeader sessionWorkspaceHeader">
        <button aria-label="Open navigation" className="iconButton mobileMenu" onClick={onOpenNavigation} type="button"><Icon name="menu" /></button>
        <div><h1>Sessions</h1></div>
        <div className="sessionHeaderActions"><label className="routeSearch"><Icon name="search" size={16} /><span className="srOnly">Search sessions</span><input onChange={(event) => setQuery(event.target.value.slice(0, 128))} placeholder="Search sessions…" type="search" value={query} /></label><Button icon="new" onClick={onCreate} variant="primary">New session</Button></div>
      </header>

      <div className="sessionFilterBar" role="group" aria-label="Session visibility">
        {([
          ["open", `Open ${openCount}`],
          ["archived", `Archived ${archivedCount}`],
          ["all", `All ${snapshot.sessions.length}`],
        ] as const).map(([value, label]) => (
          <button aria-pressed={filter === value} data-active={filter === value} key={value} onClick={() => setFilter(value)} type="button">{label}</button>
        ))}
        <label className="sessionSort"><span className="srOnly">Sort sessions</span><select aria-label="Sort sessions" value={sort} onChange={(event) => setSort(event.target.value)}><option value="recent">Recent activity</option><option value="title">Title A–Z</option></select></label>
      </div>

      <div className="sessionsWorkspaceGrid">
        <section aria-label="Session results" className="sessionResults">
          {visibleSessions.length > 0 ? (
            <VirtualList
              ariaLabel="All sessions"
              height={Math.min(listHeight, visibleSessions.length * 64)}
              itemKey={(session) => session.id}
              items={visibleSessions}
              renderItem={(session) => (
                <button
                  aria-current={session.id === selected?.id ? "true" : undefined}
                  className="sessionWorkspaceRow"
                  data-active={session.id === snapshot.activeSessionId}
                  data-selected={session.id === selected?.id}
                  onClick={() => setSelectedId(session.id)}
                  onDoubleClick={() => { if (!session.archived) onOpen(session.id); }}
                  onKeyDown={(event) => { if (event.key === "Enter" && !session.archived) { event.preventDefault(); onOpen(session.id); } }}
                  title={session.archived ? session.title : `${session.title} · Enter or double-click to open`}
                  type="button"
                >
                  <span className="sessionWorkspaceIcon"><Icon name={session.archived ? "database" : "chat"} /></span>
                  <span className="sessionWorkspaceCopy"><strong>{session.title}</strong><small>{session.messageCount === undefined ? "Message count unavailable" : `${session.messageCount} messages`}{session.archived ? " · Archived" : session.id === snapshot.activeSessionId ? " · Current" : ""}</small></span>
                  {timestampStyle !== "hidden" && session.updatedAt ? <time dateTime={session.updatedAt}>{formattedDate(session.updatedAt, timestampStyle)}</time> : <span />}
                  <Icon name="chevron" />
                </button>
              )}
              rowHeight={64}
            />
          ) : <p className="emptySessionSearch">No {filter === "all" ? "" : `${filter} `}sessions match “{query}”.</p>}
        </section>

        <aside aria-label="Selected session details" className="sessionDetailPane">
          {selected ? (
            <>
              <header>
                <span className="sessionDetailIcon"><Icon name={selected.archived ? "database" : "sessions"} /></span>
                <div><p className="eyebrow">{selected.archived ? "Archived session" : selected.id === snapshot.activeSessionId ? "Active session" : "Open session"}</p><h2>{selected.title}</h2></div>
              </header>

              <div className="sessionPrimaryActions">
                {selected.archived ? (
                  <Button
                    icon="refresh"
                    loading={busyAction === "unarchive"}
                    loadingLabel="Restoring"
                    onClick={() => void run("unarchive", { type: "unarchive_session", sessionId: selected.id }, `Restored “${selected.title}”.`, false)}
                    variant="primary"
                  >Restore session</Button>
                ) : (
                  <Button icon="chat" onClick={() => onOpen(selected.id)} variant="primary">{selected.id === snapshot.activeSessionId ? "Return to chat" : "Open session"}</Button>
                )}
                <Button aria-label="Export Markdown" icon="download" onClick={() => void run("export", { type: "export_transcript", sessionId: selected.id }, `Exported “${selected.title}”.`, false)} loading={busyAction === "export"} loadingLabel="Exporting">Export</Button>
              </div>
              <div className="sessionSecondaryActions">
                <Button disabled={Boolean(busyAction)} onClick={() => openDialog({ kind: "rename", session: selected })} size="small" variant="quiet">Rename</Button>
                {!selected.archived ? <Button disabled={Boolean(busyAction)} onClick={() => openDialog({ kind: "archive", session: selected })} size="small" variant="quiet">Archive</Button> : null}
                <Button className="dangerText" disabled={Boolean(busyAction)} onClick={() => openDialog({ kind: "delete", session: selected })} size="small" variant="quiet">Delete</Button>
              </div>
              <details className="sessionMetadata"><summary>Session info</summary>
              <dl className="sessionFacts">
                <div><dt>Identity</dt><dd><code>{selected.id}</code></dd></div>
                <div><dt>Last update</dt><dd>{formattedDate(selected.updatedAt, timestampStyle)}</dd></div>
                <div><dt>Transcript</dt><dd>{selected.messageCount === undefined ? "Unknown length" : `${selected.messageCount} messages`}</dd></div>
                <div><dt>Status</dt><dd>{selected.archived ? "Read-only archive" : selected.id === snapshot.activeSessionId ? "Open in Chat" : "Available to resume"}</dd></div>
              </dl>
              </details>
              <p aria-live="polite" className="sessionActionMessage">{actionMessage}</p>
            </>
          ) : <div className="emptySessionDetail"><Icon name="sessions" /><p>Select a session to see its details.</p></div>}
        </aside>
      </div>

      {dialog?.kind === "rename" && !snapshot.pendingPermission ? createPortal(
        <Dialog
          description={`Give this session a name you can find later.`}
          footer={<><Button onClick={() => setDialog(undefined)} variant="quiet">Cancel</Button><Button disabled={Boolean(renameError) || renameTitle === dialog.session.title} loading={busyAction === "rename"} loadingLabel="Renaming" onClick={() => void run("rename", { type: "rename_session", sessionId: dialog.session.id, title: renameTitle }, `Renamed session to “${renameTitle}”.`)} variant="primary">Save title</Button></>}
          onClose={() => setDialog(undefined)}
          title={`Rename “${dialog.session.title}”`}
        >
          <Field autoComplete="off" data-initial-focus error={renameError} label="New title" onChange={(event) => setRenameTitle(event.target.value)} value={renameTitle} />
        </Dialog>, document.querySelector(".app") ?? document.body,
      ) : null}

      {dialog?.kind === "archive" && !snapshot.pendingPermission ? createPortal(
        <Dialog
          description="Archived sessions are read-only. You can restore them anytime."
          eyebrow="Confirm scope"
          footer={<><Button onClick={() => setDialog(undefined)} variant="quiet">Keep open</Button><Button loading={busyAction === "archive"} loadingLabel="Archiving" onClick={() => void run("archive", { type: "archive_session", sessionId: dialog.session.id }, `Archived “${dialog.session.title}”.`)} variant="primary">Archive this session</Button></>}
          onClose={() => setDialog(undefined)}
          title={`Archive “${dialog.session.title}”?`}
        >
          <div className="sessionScope"><span>Session identity</span><code>{dialog.session.id}</code><span>Durable messages retained</span><strong>{dialog.session.messageCount ?? "Unknown"}</strong></div>
        </Dialog>, document.querySelector(".app") ?? document.body,
      ) : null}

      {dialog?.kind === "delete" && !snapshot.pendingPermission ? createPortal(
        <Dialog
          description="AutoHarness exports this session before permanently deleting its local history. This action cannot be undone from the application."
          eyebrow="Permanent deletion"
          footer={<><Button onClick={() => setDialog(undefined)} variant="quiet">Cancel</Button><Button disabled={deleteConfirmation !== dialog.session.title} loading={busyAction === "delete"} loadingLabel="Deleting" onClick={() => void run("delete", { type: "delete_session", sessionId: dialog.session.id }, `Deleted “${dialog.session.title}”.`)} variant="danger">Delete permanently</Button></>}
          onClose={() => setDialog(undefined)}
          title={`Delete “${dialog.session.title}”?`}
        >
          <div className="sessionScope dangerScope"><span>Exact session</span><code>{dialog.session.id}</code><span>Consequence</span><strong>Export, then remove durable history</strong></div>
          <Field autoComplete="off" data-initial-focus hint={`Type “${dialog.session.title}” to confirm this exact scope.`} label="Confirm session title" onChange={(event) => setDeleteConfirmation(event.target.value)} value={deleteConfirmation} />
        </Dialog>, document.querySelector(".app") ?? document.body,
      ) : null}
    </main>
  );
}

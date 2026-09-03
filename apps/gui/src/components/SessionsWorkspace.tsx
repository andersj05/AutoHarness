import { useEffect, useMemo, useState } from "react";
import {
  MAX_SESSION_TITLE_UTF8_BYTES,
  type ClientCommand,
  type ClientSnapshot,
  type CommandOutcome,
  type SessionSummary,
  type TimestampStyle,
} from "../protocol";
import { Icon } from "./Icon";
import { Button, Chip, Dialog, Field, VirtualList } from "./primitives";

type SessionFilter = "open" | "archived" | "all";
type SessionDialog =
  | { kind: "rename"; session: SessionSummary }
  | { kind: "archive"; session: SessionSummary }
  | { kind: "delete"; session: SessionSummary };

interface SessionsWorkspaceProps {
  snapshot: ClientSnapshot;
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

export function SessionsWorkspace({ snapshot, onCommand, onOpen, onOpenNavigation, timestampStyle }: SessionsWorkspaceProps) {
  const [query, setQuery] = useState("");
  const [filter, setFilter] = useState<SessionFilter>("open");
  const [selectedId, setSelectedId] = useState(() => snapshot.activeSessionId ?? snapshot.sessions[0]?.id);
  const [dialog, setDialog] = useState<SessionDialog>();
  const [renameTitle, setRenameTitle] = useState("");
  const [deleteConfirmation, setDeleteConfirmation] = useState("");
  const [busyAction, setBusyAction] = useState<string>();
  const [actionMessage, setActionMessage] = useState<string>();
  const [listHeight, setListHeight] = useState(() => Math.max(220, Math.min(650, window.innerHeight - 255)));

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
    });
  }, [filter, query, snapshot.sessions]);
  const selected = snapshot.sessions.find((session) => session.id === selectedId);
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
      if (closeDialog) setDialog(undefined);
      setDeleteConfirmation("");
    }
  };

  const openDialog = (next: SessionDialog) => {
    setDialog(next);
    setRenameTitle(next.session.title);
    setDeleteConfirmation("");
  };

  return (
    <main className="routeWorkspace sessionRouteWorkspace" id="main-content" tabIndex={-1}>
      <header className="routeWorkspaceHeader sessionWorkspaceHeader">
        <button aria-label="Open navigation" className="iconButton mobileMenu" onClick={onOpenNavigation} type="button"><Icon name="menu" /></button>
        <div><p className="eyebrow">Durable history</p><h1>Sessions</h1><p>Search, resume, rename, archive, export, or remove replayable conversations.</p></div>
        <label className="routeSearch"><Icon name="search" size={16} /><span className="srOnly">Search sessions</span><input onChange={(event) => setQuery(event.target.value.slice(0, 128))} placeholder="Search title or identity" type="search" value={query} /></label>
      </header>

      <div className="sessionFilterBar" role="group" aria-label="Session visibility">
        {([
          ["open", `Open ${openCount}`],
          ["archived", `Archived ${archivedCount}`],
          ["all", `All ${snapshot.sessions.length}`],
        ] as const).map(([value, label]) => (
          <button aria-pressed={filter === value} data-active={filter === value} key={value} onClick={() => setFilter(value)} type="button">{label}</button>
        ))}
      </div>

      <div className="sessionsWorkspaceGrid">
        <section aria-label="Session results" className="sessionResults">
          {visibleSessions.length > 0 ? (
            <VirtualList
              ariaLabel="All sessions"
              height={listHeight}
              itemKey={(session) => session.id}
              items={visibleSessions}
              renderItem={(session) => (
                <button
                  aria-current={session.id === selectedId ? "true" : undefined}
                  className="sessionWorkspaceRow"
                  data-active={session.id === snapshot.activeSessionId}
                  data-selected={session.id === selectedId}
                  onClick={() => setSelectedId(session.id)}
                  type="button"
                >
                  <span className="sessionWorkspaceIcon"><Icon name={session.archived ? "database" : "chat"} /></span>
                  <span className="sessionWorkspaceCopy"><strong>{session.title}</strong><small>{session.messageCount === undefined ? "Message count unavailable" : `${session.messageCount} messages`}</small></span>
                  {session.archived ? <Chip intent="neutral">archived</Chip> : session.id === snapshot.activeSessionId ? <Chip icon="bolt" intent="info">active</Chip> : null}
                  {timestampStyle !== "hidden" && session.updatedAt ? <time dateTime={session.updatedAt}>{formattedDate(session.updatedAt, timestampStyle)}</time> : <span />}
                  <Icon name="chevron" />
                </button>
              )}
              rowHeight={68}
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
              <dl className="sessionFacts">
                <div><dt>Identity</dt><dd><code>{selected.id}</code></dd></div>
                <div><dt>Last update</dt><dd>{formattedDate(selected.updatedAt, timestampStyle)}</dd></div>
                <div><dt>Transcript</dt><dd>{selected.messageCount === undefined ? "Unknown length" : `${selected.messageCount} durable messages`}</dd></div>
                <div><dt>Status</dt><dd>{selected.archived ? "Read-only archive" : selected.id === snapshot.activeSessionId ? "Open in Chat" : "Available to resume"}</dd></div>
              </dl>
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
                <Button icon="copy" onClick={() => void run("export", { type: "export_transcript", sessionId: selected.id }, `Exported “${selected.title}”.`, false)} loading={busyAction === "export"} loadingLabel="Exporting">Export Markdown</Button>
              </div>
              <div className="sessionSecondaryActions">
                <Button onClick={() => openDialog({ kind: "rename", session: selected })} size="small" variant="quiet">Rename</Button>
                {!selected.archived ? <Button onClick={() => openDialog({ kind: "archive", session: selected })} size="small" variant="quiet">Archive</Button> : null}
                <Button className="dangerText" onClick={() => openDialog({ kind: "delete", session: selected })} size="small" variant="quiet">Delete</Button>
              </div>
              <p aria-live="polite" className="sessionActionMessage">{actionMessage}</p>
            </>
          ) : <div className="emptySessionDetail"><Icon name="sessions" /><p>Select a session to inspect its exact scope and available actions.</p></div>}
        </aside>
      </div>

      {dialog?.kind === "rename" ? (
        <Dialog
          description={`Rename only session ${dialog.session.id}. Its transcript and identity remain unchanged.`}
          eyebrow="Session title"
          footer={<><Button onClick={() => setDialog(undefined)} variant="quiet">Cancel</Button><Button disabled={Boolean(renameError) || renameTitle === dialog.session.title} loading={busyAction === "rename"} loadingLabel="Renaming" onClick={() => void run("rename", { type: "rename_session", sessionId: dialog.session.id, title: renameTitle }, `Renamed session to “${renameTitle}”.`)} variant="primary">Save title</Button></>}
          onClose={() => setDialog(undefined)}
          title={`Rename “${dialog.session.title}”`}
        >
          <Field autoComplete="off" data-initial-focus error={renameError} label="New title" onChange={(event) => setRenameTitle(event.target.value)} value={renameTitle} />
        </Dialog>
      ) : null}

      {dialog?.kind === "archive" ? (
        <Dialog
          description="Archiving keeps every durable event but makes this session read-only until you restore it."
          eyebrow="Confirm scope"
          footer={<><Button onClick={() => setDialog(undefined)} variant="quiet">Keep open</Button><Button loading={busyAction === "archive"} loadingLabel="Archiving" onClick={() => void run("archive", { type: "archive_session", sessionId: dialog.session.id }, `Archived “${dialog.session.title}”.`)} variant="primary">Archive this session</Button></>}
          onClose={() => setDialog(undefined)}
          title={`Archive “${dialog.session.title}”?`}
        >
          <div className="sessionScope"><span>Session identity</span><code>{dialog.session.id}</code><span>Durable messages retained</span><strong>{dialog.session.messageCount ?? "Unknown"}</strong></div>
        </Dialog>
      ) : null}

      {dialog?.kind === "delete" ? (
        <Dialog
          description="AutoHarness exports this session before permanently deleting its local history. This action cannot be undone from the application."
          eyebrow="Permanent deletion"
          footer={<><Button onClick={() => setDialog(undefined)} variant="quiet">Cancel</Button><Button disabled={deleteConfirmation !== dialog.session.title} loading={busyAction === "delete"} loadingLabel="Deleting" onClick={() => void run("delete", { type: "delete_session", sessionId: dialog.session.id }, `Deleted “${dialog.session.title}”.`)} variant="danger">Delete permanently</Button></>}
          onClose={() => setDialog(undefined)}
          title={`Delete “${dialog.session.title}”?`}
        >
          <div className="sessionScope dangerScope"><span>Exact session</span><code>{dialog.session.id}</code><span>Consequence</span><strong>Export, then remove durable history</strong></div>
          <Field autoComplete="off" data-initial-focus hint={`Type “${dialog.session.title}” to confirm this exact scope.`} label="Confirm session title" onChange={(event) => setDeleteConfirmation(event.target.value)} value={deleteConfirmation} />
        </Dialog>
      ) : null}
    </main>
  );
}

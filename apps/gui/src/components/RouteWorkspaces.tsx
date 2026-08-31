import { useMemo, useState } from "react";
import type { ClientSnapshot } from "../protocol";
import { Icon } from "./Icon";

interface SessionsWorkspaceProps {
  snapshot: ClientSnapshot;
  onOpen: (id: string) => void;
  onOpenNavigation: () => void;
}

export function SessionsWorkspace({ snapshot, onOpen, onOpenNavigation }: SessionsWorkspaceProps) {
  const [query, setQuery] = useState("");
  const visibleSessions = useMemo(() => {
    const needle = query.trim().toLocaleLowerCase();
    return needle ? snapshot.sessions.filter((session) => session.title.toLocaleLowerCase().includes(needle)) : snapshot.sessions;
  }, [query, snapshot.sessions]);
  return (
    <main className="routeWorkspace" id="main-content">
      <header className="routeWorkspaceHeader">
        <button aria-label="Open navigation" className="iconButton mobileMenu" onClick={onOpenNavigation} type="button"><Icon name="menu" /></button>
        <div><p className="eyebrow">Durable history</p><h1>Sessions</h1><p>Search, resume, and inspect every replayable conversation.</p></div>
        <label className="routeSearch"><Icon name="search" size={16} /><span className="srOnly">Search sessions</span><input onChange={(event) => setQuery(event.target.value.slice(0, 128))} placeholder="Search sessions" type="search" value={query} /></label>
      </header>
      <section className="sessionWorkspaceList" aria-label="All sessions">
        {visibleSessions.map((session) => (
          <button className="sessionWorkspaceRow" data-active={session.id === snapshot.activeSessionId} key={session.id} onClick={() => onOpen(session.id)} type="button">
            <span className="sessionWorkspaceIcon"><Icon name="chat" /></span>
            <span className="sessionWorkspaceCopy"><strong>{session.title}</strong><small>{session.messageCount === undefined ? "Message count unavailable" : `${session.messageCount} messages`}</small></span>
            {session.id === snapshot.activeSessionId ? <span className="statusChip"><Icon name="bolt" size={12} /> active</span> : null}
            {session.updatedAt ? <time dateTime={session.updatedAt}>{new Intl.DateTimeFormat(undefined, { month: "short", day: "numeric" }).format(new Date(session.updatedAt))}</time> : <span />}
            <Icon name="chevron" />
          </button>
        ))}
        {visibleSessions.length === 0 ? <p className="emptySessionSearch">No sessions match “{query}”.</p> : null}
      </section>
    </main>
  );
}

interface SimpleWorkspaceProps {
  route: "memory" | "settings";
  highContrast: boolean;
  reduceMotion: boolean;
  onHighContrast: (value: boolean) => void;
  onOpenNavigation: () => void;
  onReduceMotion: (value: boolean) => void;
}

export function SimpleWorkspace({ route, highContrast, reduceMotion, onHighContrast, onOpenNavigation, onReduceMotion }: SimpleWorkspaceProps) {
  const memory = route === "memory";
  return (
    <main className="routeWorkspace" id="main-content">
      <header className="routeWorkspaceHeader simple">
        <button aria-label="Open navigation" className="iconButton mobileMenu" onClick={onOpenNavigation} type="button"><Icon name="menu" /></button>
        <div><p className="eyebrow">{memory ? "Knowledge ledger" : "Personalization"}</p><h1>{memory ? "Memory" : "Settings"}</h1><p>{memory ? "Provenance-rich memory remains under durable host authority." : "Tune the presentation without changing runtime policy."}</p></div>
      </header>
      {memory ? (
        <section className="futureWorkspace">
          <span className="futureIcon"><Icon name="memory" size={27} /></span>
          <div><p className="eyebrow">Migration stage 7</p><h2>Memory deserves a richer canvas</h2><p>The desktop surface will add provenance timelines, relation views, admission history, and safe diffs while preserving review-only proposals.</p></div>
          <div className="futurePreview" aria-hidden="true"><i /><i /><i /><i /></div>
        </section>
      ) : (
        <section className="settingsWorkspace" aria-labelledby="appearance-heading">
          <div><p className="eyebrow">Interface</p><h2 id="appearance-heading">Appearance and motion</h2><p>System preferences remain the default. These preview controls are presentation-only.</p></div>
          <label className="settingRow"><span><strong>High contrast</strong><small>Increase outlines and remove translucent surfaces.</small></span><input checked={highContrast} onChange={(event) => onHighContrast(event.target.checked)} type="checkbox" /></label>
          <label className="settingRow"><span><strong>Reduce motion</strong><small>Freeze looping activity and remove spatial transitions.</small></span><input checked={reduceMotion} onChange={(event) => onReduceMotion(event.target.checked)} type="checkbox" /></label>
          <div className="themePreview"><span /><span /><span /><strong>System dark</strong><small>#080c18 · cyan · violet</small></div>
        </section>
      )}
    </main>
  );
}

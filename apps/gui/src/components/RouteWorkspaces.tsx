import { Icon } from "./Icon";

interface MemoryWorkspaceProps {
  onOpenNavigation: () => void;
}

export function MemoryWorkspace({ onOpenNavigation }: MemoryWorkspaceProps) {
  return (
    <main className="routeWorkspace" id="main-content" tabIndex={-1}>
      <header className="routeWorkspaceHeader simple">
        <button aria-label="Open navigation" className="iconButton mobileMenu" onClick={onOpenNavigation} type="button"><Icon name="menu" /></button>
        <div><p className="eyebrow">Knowledge ledger</p><h1>Memory</h1><p>Provenance-rich memory remains under durable host authority.</p></div>
      </header>
      <section className="futureWorkspace">
        <span className="futureIcon"><Icon name="memory" size={27} /></span>
        <div><p className="eyebrow">Migration stage 7</p><h2>Memory deserves a richer canvas</h2><p>The desktop surface will add provenance timelines, relation views, admission history, and safe diffs while preserving review-only proposals.</p></div>
        <div className="futurePreview" aria-hidden="true"><i /><i /><i /><i /></div>
      </section>
    </main>
  );
}

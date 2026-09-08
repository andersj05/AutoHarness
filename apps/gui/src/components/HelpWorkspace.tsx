import { useState } from "react";
import type { RouteId } from "./AppRail";
import { Icon, type IconName } from "./Icon";
import { Button } from "./primitives";

interface Topic {
  id: string;
  title: string;
  icon: IconName;
  paragraphs: readonly string[];
  steps?: readonly string[];
  route?: RouteId;
  action?: string;
}

const topics: readonly Topic[] = [
  { id: "start", title: "Start a conversation", icon: "chat", paragraphs: ["Connect a provider once, then choose a model and start writing."], steps: ["Open Providers. Add a Gemini or router profile, or connect your Codex subscription.", "Activate the connection and choose a model in Chat.", "Write your prompt and send it. Use Stop to cancel a response, or Retry after a retryable failure."], route: "providers", action: "Open Providers" },
  { id: "sessions", title: "Sessions and exports", icon: "sessions", paragraphs: ["Each session keeps its own conversation and unfinished draft. New session starts a separate conversation. A Draft label in the sidebar marks unfinished text.", "Click a conversation title to rename it. Use Sessions to search, sort, archive, or restore conversations. Archived sessions are read-only until restored.", "The conversation menu can copy text or export Markdown. Permanent deletion requires the complete session title and exports its history before removal."], route: "sessions", action: "Browse Sessions" },
  { id: "providers", title: "Providers and credentials", icon: "providers", paragraphs: ["A provider profile stores connection settings and model defaults. Test connection checks whether that profile can connect. Make active uses it for your work.", "Session-only credentials are temporary. Saved credentials use your system credential vault. An environment credential takes precedence over a saved credential. Never paste credentials into Chat.", "Codex sign-in opens your browser. Finish signing in there, then return to AutoHarness. You can cancel the sign-in from Providers."], route: "providers", action: "Open Providers" },
  { id: "permissions", title: "Permissions and tool activity", icon: "shield", paragraphs: ["A permission request pauses ordinary interaction. Review the exact tool, arguments, and scope before choosing Allow or Deny. An answer applies only to that call.", "Expand a tool entry in the conversation to inspect its result and evidence. Session details shows the selected model, reported token counts, and activity.", "If tool work is interrupted, inspect the recovered state before retrying. An external effect may already have happened."], route: "chat", action: "Return to Chat" },
  { id: "memory", title: "Memory and trust", icon: "memory", paragraphs: ["Memory stores instructions and knowledge for future conversations. Search by content, status, or scope. Select a record to read it, copy its content, or inspect Details and history.", "Imported documents and model-authored proposals remain untrusted until you review and approve the current version. Approval preserves the original provenance. A changed revision must be reviewed again.", "Correct edits a saved instruction. Retract prevents future use while retaining history. Export and Delete content are available under More memory actions, with an explicit review before execution."], route: "memory", action: "Open Memory" },
  { id: "appearance", title: "Appearance and accessibility", icon: "settings", paragraphs: ["Choose a light, dark, or colored theme, or follow your system. Contrast controls preserve readable text and visible focus.", "Accessibility settings include interface zoom up to 200 percent, conversation text size, and reduced motion. Conversation settings control timestamps and the keyboard shortcut for sending messages.", "Tab and Shift + Tab move focus. Escape closes ordinary dialogs. Arrow keys resize focused pane separators; hold Shift for larger steps."], route: "settings", action: "Open Settings" },
  { id: "recovery", title: "Offline and restart recovery", icon: "refresh", paragraphs: ["You can read saved sessions offline. Check your provider connection and refresh the model catalog before sending again.", "If AutoHarness loses its runtime connection, restart the application to restore saved state. An unacknowledged renderer replacement requires a process restart.", "If a prompt's outcome is uncertain, wait for recovery before resending it. Use Quit AutoHarness in Search for an orderly shutdown."], route: "providers", action: "Check Providers" },
  { id: "updates", title: "Updates and rollback", icon: "download", paragraphs: ["This desktop is a development preview. Updates use deliberate installer changes.", "Close all clients and keep a cold backup of the entire application data directory before upgrading. Roll back with the previous package and untouched backup. Do not open a migrated live database with an older application.", "Both application shortcuts open the desktop."] },
  { id: "shortcuts", title: "Keyboard shortcuts", icon: "command", paragraphs: ["Use these shortcuts from the main workspace. Dialogs keep keyboard focus until you close them."] },
];

export function HelpWorkspace({ onOpenNavigation, onRoute }: { onOpenNavigation: () => void; onRoute: (route: RouteId) => void }) {
  const [query, setQuery] = useState("");
  const [selectedId, setSelectedId] = useState("start");
  const needle = query.trim().toLocaleLowerCase();
  const visible = topics.filter((topic) => `${topic.title} ${topic.paragraphs.join(" ")} ${topic.steps?.join(" ") ?? ""}`.toLocaleLowerCase().includes(needle));
  const selected = visible.find((topic) => topic.id === selectedId) ?? visible[0];
  return (
    <main aria-label="Help" className="routeWorkspace helpWorkspace" id="main-content" tabIndex={-1}>
      <header className="routeWorkspaceHeader settingsWorkspaceHeader">
        <button aria-label="Open navigation" className="iconButton mobileMenu" onClick={onOpenNavigation} type="button"><Icon name="menu" /></button>
        <div><h1>Help</h1></div>
        <label className="routeSearch"><Icon name="search" size={16} /><span className="srOnly">Search help</span><input onChange={(event) => setQuery(event.target.value.slice(0, 128))} placeholder="Search help" type="search" value={query} /></label>
      </header>
      <div className="helpLayout">
        <nav className="helpTopicNav" aria-label="Help topics">
          {visible.map((topic) => <button key={topic.id} type="button" aria-current={selected?.id === topic.id ? "true" : undefined} onClick={() => setSelectedId(topic.id)}><Icon name={topic.icon} size={16} /><span>{topic.title}</span></button>)}
        </nav>
        {visible.length ? <label className="helpTopicSelect">Help topic<select value={selected?.id} onChange={(event) => setSelectedId(event.target.value)}>{visible.map((topic) => <option key={topic.id} value={topic.id}>{topic.title}</option>)}</select></label> : null}
        {selected ? <article className="helpArticle" aria-labelledby="help-article-title" key={selected.id}>
          <h2 id="help-article-title">{selected.title}</h2>
          {selected.paragraphs.map((paragraph) => <p key={paragraph}>{paragraph}</p>)}
          {selected.steps ? <ol className="helpSteps">{selected.steps.map((step, index) => <li key={step}><span className="helpStepNumber" aria-hidden="true">{index + 1}</span><span>{step}</span></li>)}</ol> : null}
          {selected.route ? <Button icon={selected.route === "help" ? "inspect" : selected.route} onClick={() => { if (selected.route) onRoute(selected.route); }}>{selected.action}</Button> : null}
          {selected.id === "shortcuts" ? <dl className="helpShortcuts">
            <div><dt>Search and commands</dt><dd><kbd>Ctrl/Cmd + K</kbd></dd></div>
            <div><dt>New session</dt><dd><kbd>Ctrl/Cmd + N</kbd></dd></div>
            <div><dt>Find in transcript</dt><dd><kbd>Ctrl/Cmd + F</kbd></dd></div>
            <div><dt>Chat / Sessions / Providers</dt><dd><kbd>Alt + 1 / 2 / 3</kbd></dd></div>
            <div><dt>Memory / Settings / Help</dt><dd><kbd>Alt + 4 / 5 / 6</kbd></dd></div>
            <div><dt>Help</dt><dd><kbd>F1</kbd></dd></div>
            <div><dt>Send message (set in Settings)</dt><dd><kbd>Enter</kbd> or <kbd>Ctrl/Cmd + S</kbd></dd></div>
            <div><dt>New line in a message</dt><dd><kbd>Shift + Enter</kbd></dd></div>
          </dl> : null}
        </article> : <div className="helpEmpty"><Icon name="search" size={24} /><h2>No matching topics</h2><p>Try sessions, credentials, or recovery.</p><Button variant="quiet" onClick={() => setQuery("")}>Clear search</Button></div>}
      </div>
      <span role="status" className="srOnly">{needle ? `${visible.length} matching help topics` : ""}</span>
    </main>
  );
}

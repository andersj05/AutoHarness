import { useState } from "react";
import { Icon } from "./Icon";

const topics = [
  { title: "Start a conversation", text: "Open Providers to create and activate a Gemini or router connection, or sign in with your Codex subscription. Choose a compatible model in Chat. Write a prompt and use Send. Settings controls whether Enter or Ctrl/Cmd + S submits; Shift + Enter inserts a new line. Stop cancels active work. Retry is available for retryable failures." },
  { title: "Sessions and exports", text: "Create a session with Ctrl/Cmd + N. Sessions searches open and archived conversations. Select a session to rename, archive, restore, export, or delete it. Deletion requires its complete title and exports durable history before removal. Chat search (Ctrl/Cmd + F) finds messages and tool evidence. Copy copies text; Export writes the durable transcript." },
  { title: "Providers and credentials", text: "Providers manages named connections, model defaults, and reasoning effort. Session-only credentials are temporary. Save or replace uses the operating-system vault. An environment credential takes precedence over a saved vault credential. Follow the displayed recovery guidance if the vault is unavailable. Never paste credentials into Chat. Codex sign-in opens your browser and can be cancelled inside Providers." },
  { title: "Permissions and tool activity", text: "A permission request interrupts ordinary interaction. Review the exact tool, arguments, and scope before choosing Allow or Deny. Denial does not grant authority to execute. Tool disclosures show results and evidence. If interrupted during tool work, inspect the recovered state before retrying because an external effect may already have happened." },
  { title: "Memory and trust", text: "Memory searches and filters records and shows provenance, evidence, relations, and admission history. Imported and model-authored proposals remain untrusted until explicitly approved. Review the current revision before approval. Corrections, retractions, export, and exact confirmed deletion are available from the selected record. A stale review must be refreshed before approval." },
  { title: "Appearance and accessibility", text: "Settings exposes theme, contrast, interface zoom up to 200 percent, conversation font size, reduced motion, timestamps, and submission behavior. Each preference explains its effective source and can reset a saved override. Tab and Shift + Tab move focus; Escape dismisses ordinary dialogs. Arrow keys resize focused pane separators, with Shift for larger steps." },
  { title: "Offline and restart recovery", text: "Offline sessions remain available without a provider connection. Check Providers and refresh the model catalog before sending again. If the desktop loses its runtime connection, restart the application to restore durable state. An unacknowledged renderer replacement requires a process restart. Do not resend a prompt whose outcome is uncertain until recovery settles. Use Quit AutoHarness in the command palette for an orderly shutdown." },
  { title: "Updates and rollback", text: "This desktop is a development preview. Updates are deliberate installer changes. Close all clients and keep a cold backup of the entire application data directory before upgrading. Roll back with the previous package and untouched backup; do not open a migrated live database with an older application. Both application shortcuts open the desktop." },
];

export function HelpWorkspace({ onOpenNavigation }: { onOpenNavigation: () => void }) {
  const [query, setQuery] = useState("");
  const visible = topics.filter((topic) => `${topic.title} ${topic.text}`.toLowerCase().includes(query.trim().toLowerCase()));
  return (
    <main aria-label="Help" className="routeWorkspace settingsRouteWorkspace" id="main-content" tabIndex={-1}>
      <header className="routeWorkspaceHeader settingsWorkspaceHeader">
        <button aria-label="Open navigation" className="iconButton mobileMenu" onClick={onOpenNavigation} type="button"><Icon name="menu" /></button>
        <div><h1>Help</h1></div>
        <label className="routeSearch"><Icon name="search" size={16} /><span className="srOnly">Search help</span><input onChange={(event) => setQuery(event.target.value.slice(0, 128))} placeholder="Search help" type="search" value={query} /></label>
      </header>
      <div className="helpContent">
        <section aria-labelledby="help-shortcuts" className="settingsWorkspace">
          <header><h2 id="help-shortcuts">Keyboard shortcuts</h2></header>
          <dl className="helpShortcuts">
            <div><dt>Command palette</dt><dd><kbd>Ctrl/Cmd + K</kbd></dd></div>
            <div><dt>New session</dt><dd><kbd>Ctrl/Cmd + N</kbd></dd></div>
            <div><dt>Find in transcript</dt><dd><kbd>Ctrl/Cmd + F</kbd></dd></div>
            <div><dt>Chat, Sessions, Providers, Memory, Settings</dt><dd><kbd>Alt + 1 through 5</kbd></dd></div>
            <div><dt>Help</dt><dd><kbd>F1</kbd> or <kbd>Alt + 6</kbd></dd></div>
          </dl>
        </section>
        <p aria-live="polite" className="helpResults">{visible.length ? `${visible.length} ${visible.length === 1 ? "topic" : "topics"}` : "No matching topics. Try credentials, recovery, or sessions."}</p>
        {visible.map((topic) => <details className="helpTopic" key={`${topic.title}-${Boolean(query)}`} open={query.trim() ? true : undefined}><summary><h2>{topic.title}</h2><Icon name="chevron" size={16} /></summary><p>{topic.text}</p></details>)}
      </div>
    </main>
  );
}

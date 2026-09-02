import {
  CLIENT_SCHEMA_VERSION,
  type ActiveSessionProjection,
  type ClientCommand,
  type ClientFrame,
  type ClientSnapshot,
  type ClientTransport,
  type CommandReceipt,
  type EphemeralCredential,
  type SessionSummary,
  type TranscriptItem,
} from "../protocol";
import { securityDisplaySafe } from "../securityText";

export type FixtureScenario = "ready" | "streaming" | "offline" | "credential" | "permission" | "failed" | "empty";

const MODEL_CATALOG = [
  {
    id: "gemini/gemini-2.5-pro",
    displayName: "Gemini 2.5 Pro",
    provider: "Google AI Studio",
    description: "Deep reasoning, tools, and a large working context.",
    contextWindowTokens: "1048576",
    selectable: true,
    supportsTools: true,
    supportsReasoning: true,
  },
  {
    id: "codex/gpt-5.6-luna",
    displayName: "GPT-5.6 Luna",
    provider: "OpenAI subscription",
    description: "Fast agentic coding with configurable reasoning.",
    contextWindowTokens: "400000",
    selectable: true,
    supportsTools: true,
    supportsReasoning: true,
  },
  {
    id: "router/deepseek-v3.2",
    displayName: "DeepSeek V3.2",
    provider: "Local router",
    description: "Efficient general reasoning through your configured router.",
    contextWindowTokens: "128000",
    selectable: true,
    supportsTools: false,
    supportsReasoning: false,
  },
] as const;

const sessions: readonly SessionSummary[] = [
  {
    id: "session-gui-migration",
    title: "Design the GUI migration",
    updatedAt: "2026-08-30T13:46:00.000Z",
    messageCount: "7",
    archived: false,
  },
  {
    id: "session-context",
    title: "Audit context manifests",
    updatedAt: "2026-08-30T11:21:00.000Z",
    messageCount: "18",
    archived: false,
  },
  {
    id: "session-provider",
    title: "Provider recovery probes",
    updatedAt: "2026-08-29T20:08:00.000Z",
    messageCount: "12",
    archived: false,
  },
];

const baseTranscript: readonly TranscriptItem[] = [
  {
    kind: "message",
    id: "message-1",
    role: "user",
    content:
      "Map the terminal experience into a desktop workspace without weakening the durable runtime boundary.",
    createdAt: "2026-08-30T13:42:12.000Z",
  },
  {
    kind: "message",
    id: "message-2",
    role: "agent",
    content:
      "The clean migration keeps Rust authoritative and turns the interface into a projection. I would begin with three stable seams:\n\n1. A versioned command and snapshot protocol.\n2. A React-free client store that repairs revision gaps.\n3. Presentation slots that only receive typed props and callbacks.\n\nThat gives us a richer canvas without moving permissions, secrets, or recovery into the webview.",
    createdAt: "2026-08-30T13:42:19.000Z",
  },
  {
    kind: "tool",
    id: "tool-1",
    name: "workspace.read",
    summary: "Read the accepted desktop client decision",
    resource: "docs/adr/0019-use-tauri-web-rendered-desktop-client.md",
    status: "succeeded",
    detail: "18.4 KiB read through the workspace capability boundary",
  },
  {
    kind: "message",
    id: "message-3",
    role: "agent",
    content:
      "The first GUI slice can already feel complete: session rail, open conversation flow, bounded composer, model choice, live activity, and a context inspector. The fixture carrier can exercise the same contract in an ordinary browser while Tauri connects the real host.",
    createdAt: "2026-08-30T13:43:08.000Z",
  },
];

function activeSessionFor(scenario: FixtureScenario): ActiveSessionProjection {
  const attempt =
    scenario === "streaming"
      ? ({ kind: "streaming", id: "attempt-streaming", startedAt: "2026-08-30T13:46:00.000Z" } as const)
      : scenario === "failed"
        ? ({
            kind: "failed",
            id: "attempt-failed",
            code: "provider_stream_interrupted",
            message: "The provider stream ended before a durable completion arrived.",
            retryable: true,
          } as const)
        : ({ kind: "completed", id: "attempt-complete", inputTokens: "1842", outputTokens: "611" } as const);

  const transcript = [...baseTranscript];
  if (scenario === "streaming") {
    transcript.push({
      kind: "message",
      id: "message-streaming",
      role: "agent",
      content: "I am shaping the desktop shell now. The center stays calm while the surrounding panes disclose",
      createdAt: "2026-08-30T13:46:00.000Z",
      streaming: true,
    });
  }

  return {
    id: "session-gui-migration",
    revision: "14",
    title: "Design the GUI migration",
    selectedModelId: "gemini/gemini-2.5-pro",
    workspaceLabel: "~/AutoHarness",
    branchLabel: "feat/gui-application-shell",
    transcript,
    attempt,
  };
}

export function createFixtureSnapshot(scenario: FixtureScenario = "ready"): ClientSnapshot {
  const activeSession = activeSessionFor(scenario);
  const snapshot: ClientSnapshot = {
    schemaVersion: CLIENT_SCHEMA_VERSION,
    transportRevision: "1",
    runtimeMode: "fixture",
    connection:
      scenario === "credential"
        ? {
            kind: "credential_required",
            providerLabel: "Google AI Studio",
            reason: "No provider credential is available. Replay remains fully available.",
          }
        : scenario === "offline"
        ? {
            kind: "offline",
            reason: "No provider credential is available. Replay remains fully available.",
            recoverable: true,
          }
        : {
            kind: "online",
            providerLabel: "Google AI Studio",
            credentialSource: "Simulated fixture",
          },
    activeSessionId: activeSession.id,
    connectionId: "connection-gemini",
    sessions,
    catalog: {
      status: scenario === "credential" ? "credential_required" : scenario === "empty" ? "empty" : "ready",
      source: scenario === "offline" ? "fresh_cache" : scenario === "empty" ? "none" : "live",
      models: scenario === "empty" || scenario === "credential" ? [] : MODEL_CATALOG,
      refreshedAt: "2026-08-30T13:40:00.000Z",
    },
    activeSession,
    activity: [
      { id: "activity-1", label: "Fixture session", detail: "Simulated revision 14", status: "complete" },
      { id: "activity-2", label: "Fixture context", detail: "Simulated usage projection", status: "complete" },
      {
        id: "activity-3",
        label: "Provider turn",
        detail: scenario === "streaming" ? "Streaming response" : scenario === "failed" ? "Needs attention" : "Settled",
        status: scenario === "streaming" ? "active" : scenario === "failed" ? "warning" : "complete",
      },
    ],
  };

  if (scenario === "permission") {
    snapshot.pendingPermission = {
      id: "permission-42",
      sessionId: activeSession.id,
      toolName: securityDisplaySafe("workspace.write"),
      capability: securityDisplaySafe("Write one workspace file"),
      resource: securityDisplaySafe("apps/gui/src/App.tsx"),
      reason: "The agent requested permission to update the desktop application shell.",
      trustedFields: [
        { label: securityDisplaySafe("Operation"), value: securityDisplaySafe("Replace file contents") },
        { label: securityDisplaySafe("Workspace"), value: securityDisplaySafe("~/AutoHarness") },
        { label: securityDisplaySafe("Scope"), value: securityDisplaySafe("This call only") },
      ],
    };
  }

  return snapshot;
}

function cloneSnapshot(snapshot: ClientSnapshot): ClientSnapshot {
  return JSON.parse(JSON.stringify(snapshot)) as ClientSnapshot;
}

export class FixtureTransport implements ClientTransport {
  private current: ClientSnapshot;
  private listener?: (frame: ClientFrame) => void;
  private revision = 1n;
  private requestSequence = 0;
  private timers = new Set<ReturnType<typeof setTimeout>>();
  private closed = false;

  constructor(scenario: FixtureScenario = "ready") {
    this.current = createFixtureSnapshot(scenario);
  }

  async connect(onFrame: (frame: ClientFrame) => void): Promise<ClientSnapshot> {
    this.listener = onFrame;
    return cloneSnapshot(this.current);
  }

  async snapshot(): Promise<ClientSnapshot> {
    return cloneSnapshot(this.current);
  }

  async command(command: ClientCommand): Promise<CommandReceipt> {
    if (this.closed) {
      throw new Error("Fixture transport is closed");
    }
    const receipt = { requestId: `fixture-request-${++this.requestSequence}` };

    switch (command.type) {
      case "create_session":
        this.createSession();
        break;
      case "open_session":
        this.openSession(command.sessionId);
        break;
      case "rename_session":
        this.renameSession(command.sessionId, command.title);
        break;
      case "archive_session":
        this.setArchived(command.sessionId, true);
        break;
      case "unarchive_session":
        this.setArchived(command.sessionId, false);
        break;
      case "export_transcript":
        break;
      case "delete_session":
        this.deleteSession(command.sessionId);
        break;
      case "refresh_catalog":
        this.refreshCatalog();
        break;
      case "select_model":
        this.updateActiveSession((session) => ({ ...session, selectedModelId: command.modelId }));
        this.emit();
        break;
      case "submit_prompt":
        this.submitPrompt(command.prompt);
        break;
      case "cancel_attempt":
        this.cancelAttempt(command.attemptId);
        break;
      case "retry_attempt":
        this.beginStream("Retrying from the last durable user turn. ");
        break;
      case "answer_permission":
        this.answerPermission(command.decision);
        break;
    }
    this.emitNotice(receipt.requestId, "success", "command_committed", "The browser fixture accepted the simulated command.");
    return receipt;
  }

  async submitCredential(secret: EphemeralCredential): Promise<CommandReceipt> {
    const receipt = { requestId: `fixture-secret-${++this.requestSequence}` };
    if (secret.credential.length > 0) {
      this.current = {
        ...this.current,
        connection: { kind: "online", providerLabel: "Google AI Studio", credentialSource: "Session only" },
      };
      this.emit();
    }
    this.emitNotice(receipt.requestId, "success", "command_committed", "The credential entered the fixture boundary.");
    return receipt;
  }

  async close(): Promise<void> {
    this.closed = true;
    this.listener = undefined;
    this.clearTimers();
  }

  private createSession(): void {
    const id = `session-new-${this.requestSequence}`;
    const summary: SessionSummary = {
      id,
      title: "New conversation",
      updatedAt: new Date().toISOString(),
      messageCount: "0",
      archived: false,
    };
    this.current = {
      ...this.current,
      activeSessionId: id,
      sessions: [summary, ...this.current.sessions],
      activeSession: {
        id,
        revision: "1",
        title: summary.title,
        selectedModelId: this.current.catalog.models[0]?.id,
        workspaceLabel: "~/AutoHarness",
        branchLabel: "feat/gui-application-shell",
        transcript: [],
        attempt: { kind: "idle" },
      },
    };
    this.emit();
  }

  private openSession(sessionId: string): void {
    const summary = this.current.sessions.find((candidate) => candidate.id === sessionId);
    if (!summary || sessionId === this.current.activeSessionId) return;
    this.current = {
      ...this.current,
      activeSessionId: sessionId,
      activeSession: {
        ...activeSessionFor("ready"),
        id: sessionId,
        title: summary.title,
        revision: "8",
        transcript: [
          {
            kind: "message",
            id: `${sessionId}-message`,
            role: "agent",
            content: summary.messageCount === undefined
              ? "Durable replay restored this session."
              : `Durable replay restored ${summary.messageCount} messages for this session.`,
            createdAt: summary.updatedAt,
          },
        ],
      },
    };
    this.emit();
  }

  private renameSession(sessionId: string, title: string): void {
    const exactTitle = title.trim();
    if (!exactTitle) return;
    this.current = {
      ...this.current,
      sessions: this.current.sessions.map((session) => (
        session.id === sessionId ? { ...session, title: exactTitle } : session
      )),
      activeSession: this.current.activeSession?.id === sessionId
        ? { ...this.current.activeSession, title: exactTitle }
        : this.current.activeSession,
    };
    this.emit();
  }

  private setArchived(sessionId: string, archived: boolean): void {
    this.current = {
      ...this.current,
      sessions: this.current.sessions.map((session) => (
        session.id === sessionId ? { ...session, archived } : session
      )),
    };
    this.emit();
  }

  private deleteSession(sessionId: string): void {
    const sessions = this.current.sessions.filter((session) => session.id !== sessionId);
    if (sessions.length === this.current.sessions.length || sessions.length === 0) return;
    this.current = { ...this.current, sessions };
    if (this.current.activeSessionId === sessionId) {
      const replacement = sessions.find((session) => !session.archived) ?? sessions[0];
      if (replacement) this.openSession(replacement.id);
      return;
    }
    this.emit();
  }

  private refreshCatalog(): void {
    this.current = { ...this.current, catalog: { ...this.current.catalog, status: "loading" } };
    this.emit();
    this.schedule(() => {
      this.current = {
        ...this.current,
        connection: {
          kind: "online",
          providerLabel: "Google AI Studio",
          credentialSource: "Simulated fixture",
        },
        catalog: {
          status: "ready",
          source: "live",
          models: MODEL_CATALOG,
          refreshedAt: new Date().toISOString(),
        },
      };
      this.emit();
    }, 550);
  }

  private submitPrompt(prompt: string): void {
    if (!prompt.trim() || !this.current.activeSession) return;
    const item: TranscriptItem = {
      kind: "message",
      id: `message-user-${this.requestSequence}`,
      role: "user",
      content: prompt,
      createdAt: new Date().toISOString(),
    };
    this.updateActiveSession((session) => ({ ...session, transcript: [...session.transcript, item] }));
    this.beginStream("");
  }

  private beginStream(prefix: string): void {
    if (!this.current.activeSession) return;
    this.clearTimers();
    const attemptId = `attempt-${this.requestSequence}`;
    const messageId = `message-agent-${this.requestSequence}`;
    const initial: TranscriptItem = {
      kind: "message",
      id: messageId,
      role: "agent",
      content: prefix,
      createdAt: new Date().toISOString(),
      streaming: true,
    };
    this.updateActiveSession((session) => ({
      ...session,
      transcript: [...session.transcript, initial],
      attempt: { kind: "streaming", id: attemptId, startedAt: new Date().toISOString() },
    }));
    this.emit();

    const chunks = [
      "The desktop boundary is live. ",
      "Your prompt was persisted before dispatch, and this response is arriving as an ordered projection. ",
      "The same React surface can now follow either the browser fixture or the native Tauri carrier.",
    ];
    chunks.forEach((chunk, index) => {
      this.schedule(() => {
        const session = this.current.activeSession;
        if (!session || session.attempt.kind !== "streaming" || session.attempt.id !== attemptId) return;
        this.updateActiveSession((current) => ({
          ...current,
          transcript: current.transcript.map((item) =>
            item.kind === "message" && item.id === messageId ? { ...item, content: item.content + chunk } : item,
          ),
        }));
        this.emit();
      }, 320 * (index + 1));
    });
    this.schedule(() => {
      const session = this.current.activeSession;
      if (!session || session.attempt.kind !== "streaming" || session.attempt.id !== attemptId) return;
      this.updateActiveSession((current) => ({
        ...current,
        transcript: current.transcript.map((item) =>
          item.kind === "message" && item.id === messageId ? { ...item, streaming: false } : item,
        ),
        attempt: { kind: "completed", id: attemptId, inputTokens: "2041", outputTokens: "122" },
      }));
      this.emit();
    }, 1_280);
  }

  private cancelAttempt(attemptId: string): void {
    const session = this.current.activeSession;
    if (!session || session.attempt.kind !== "streaming" || session.attempt.id !== attemptId) return;
    this.clearTimers();
    this.updateActiveSession((current) => ({
      ...current,
      transcript: current.transcript.map((item) =>
        item.kind === "message" && item.streaming ? { ...item, streaming: false } : item,
      ),
      attempt: { kind: "cancelled", id: attemptId },
    }));
    this.emit();
  }

  private answerPermission(decision: "allow_once" | "deny"): void {
    if (!this.current.pendingPermission || !this.current.activeSession) return;
    const permission = this.current.pendingPermission;
    const tool: TranscriptItem = {
      kind: "tool",
      id: `tool-${permission.id}`,
      name: permission.toolName,
      summary: permission.capability,
      resource: permission.resource,
      status: decision === "allow_once" ? "succeeded" : "denied",
      detail: decision === "allow_once" ? "Allowed for this exact frozen call" : "Denied by the user",
    };
    this.current = { ...this.current, pendingPermission: undefined };
    this.updateActiveSession((session) => ({ ...session, transcript: [...session.transcript, tool] }));
    this.emit();
  }

  private updateActiveSession(update: (session: ActiveSessionProjection) => ActiveSessionProjection): void {
    if (!this.current.activeSession) return;
    const activeSession = update(this.current.activeSession);
    this.current = { ...this.current, activeSession };
  }

  private emit(): void {
    if (!this.listener || this.closed) return;
    this.revision += 1n;
    const revision = this.revision.toString();
    this.current = { ...this.current, transportRevision: revision };
    this.listener({ kind: "snapshot", reason: "projection", revision, snapshot: cloneSnapshot(this.current) });
  }

  private emitNotice(
    requestId: string,
    level: "success" | "error",
    code: string,
    message: string,
  ): void {
    if (!this.listener || this.closed) return;
    this.revision += 1n;
    const revision = this.revision.toString();
    this.current = { ...this.current, transportRevision: revision };
    this.listener({ kind: "notice", revision, requestId, level, code, message });
  }

  private schedule(callback: () => void, delay: number): void {
    const timer = setTimeout(() => {
      this.timers.delete(timer);
      callback();
    }, delay);
    this.timers.add(timer);
  }

  private clearTimers(): void {
    this.timers.forEach((timer) => clearTimeout(timer));
    this.timers.clear();
  }
}

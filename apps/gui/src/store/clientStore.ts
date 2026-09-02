import type {
  ClientCommand,
  ClientFrame,
  ClientSnapshot,
  ClientTransport,
  CommandOutcome,
  CommandReceipt,
  CredentialSubmission,
  NoticeLevel,
  TranscriptItem,
} from "../protocol";
import { CLIENT_SCHEMA_VERSION } from "../protocol";

export interface ClientNotice {
  level: NoticeLevel;
  code: string;
  message: string;
}

export interface ClientStoreSnapshot {
  lifecycle: "booting" | "ready" | "resyncing" | "failed";
  projection?: ClientSnapshot;
  transportRevision?: string;
  notice?: ClientNotice;
  commandError?: string;
  optimisticPrompts: readonly OptimisticPrompt[];
}

export interface OptimisticPrompt {
  requestId: string;
  sessionId: string;
  content: string;
}

interface TrackedOptimisticPrompt extends OptimisticPrompt {
  baselineUserMessageIds: ReadonlySet<string>;
}

type StoreListener = () => void;

function revisionValue(revision: string): bigint {
  if (!/^[1-9]\d*$/.test(revision)) {
    throw new Error("The host published an invalid transport revision");
  }
  const value = BigInt(revision);
  if (value > 18_446_744_073_709_551_615n) {
    throw new Error("The host published a transport revision outside u64 range");
  }
  return value;
}

function validateSnapshot(snapshot: ClientSnapshot): void {
  if (snapshot.schemaVersion !== CLIENT_SCHEMA_VERSION) {
    throw new Error(`Unsupported GUI schema ${String(snapshot.schemaVersion)}`);
  }
  revisionValue(snapshot.transportRevision);
}

export class ClientStore {
  private readonly listeners = new Set<StoreListener>();
  private state: ClientStoreSnapshot = { lifecycle: "booting", optimisticPrompts: [] };
  private started = false;
  private closed = false;
  private resync?: Promise<void>;
  private readonly commandSettlements = new Map<string, boolean>();
  private readonly commandWaiters = new Map<string, (outcome: CommandOutcome) => void>();
  private readonly optimisticPrompts = new Map<string, TrackedOptimisticPrompt>();

  constructor(
    private readonly transport: ClientTransport,
    private readonly commandSettlementTimeoutMs = 12_000,
  ) {}

  readonly getSnapshot = (): ClientStoreSnapshot => this.state;

  readonly subscribe = (listener: StoreListener): (() => void) => {
    this.listeners.add(listener);
    return () => this.listeners.delete(listener);
  };

  async start(): Promise<void> {
    if (this.started) return;
    this.started = true;
    try {
      const baseline = await this.transport.connect(
        (frame) => this.applyFrame(frame),
        (error) => this.fail(error),
      );
      if (this.closed || this.state.lifecycle === "failed") return;
      validateSnapshot(baseline);
      const currentRevision = this.state.transportRevision
        ? revisionValue(this.state.transportRevision)
        : undefined;
      if (!currentRevision || revisionValue(baseline.transportRevision) >= currentRevision) {
        this.publish({
          lifecycle: "ready",
          projection: baseline,
          transportRevision: baseline.transportRevision,
          optimisticPrompts: [],
        });
      }
    } catch (error) {
      this.fail(error);
    }
  }

  async close(): Promise<void> {
    this.closed = true;
    this.commandWaiters.forEach((waiter) => waiter("unknown"));
    this.commandWaiters.clear();
    this.commandSettlements.clear();
    this.optimisticPrompts.clear();
    await this.transport.close();
    this.listeners.clear();
  }

  async dispatch(command: ClientCommand): Promise<CommandReceipt | undefined> {
    if (this.closed || this.state.lifecycle !== "ready") return undefined;
    this.publish({ ...this.state, commandError: undefined });
    try {
      return await this.transport.command(command);
    } catch (error) {
      const message = error instanceof Error ? error.message : "The host rejected the command";
      this.publish({ ...this.state, commandError: message });
      return undefined;
    }
  }

  async dispatchAndWait(command: ClientCommand): Promise<CommandOutcome> {
    if (this.closed || this.state.lifecycle !== "ready") return "rejected";
    this.publish({ ...this.state, commandError: undefined });
    const optimistic = command.type === "submit_prompt"
      ? {
          sessionId: command.sessionId,
          content: command.prompt,
          baselineUserMessageIds: new Set(
            this.state.projection?.activeSession?.id === command.sessionId
              ? this.state.projection.activeSession.transcript
                  .filter((item) => item.kind === "message" && item.role === "user")
                  .map((item) => item.id)
              : [],
          ),
        }
      : undefined;
    let receipt: CommandReceipt;
    try {
      receipt = await this.transport.command(command);
    } catch (error) {
      const message = error instanceof Error ? error.message : "The host rejected the command";
      const outcome: CommandOutcome = this.getSnapshot().lifecycle === "failed" ? "unknown" : "rejected";
      this.publish({ ...this.state, commandError: message });
      return outcome;
    }
    if (this.closed || this.state.lifecycle !== "ready") return "unknown";
    if (optimistic) this.trackOptimisticPrompt(receipt.requestId, optimistic);
    const observed = this.commandSettlements.get(receipt.requestId);
    if (observed !== undefined) {
      this.commandSettlements.delete(receipt.requestId);
      return observed ? "committed" : "rejected";
    }
    return new Promise<CommandOutcome>((resolve) => {
      const timeout = setTimeout(() => {
        this.commandWaiters.delete(receipt.requestId);
        this.publish({
          ...this.state,
          commandError: "The host did not confirm whether the command committed.",
        });
        resolve("unknown");
        void this.requestResync();
      }, this.commandSettlementTimeoutMs);
      this.commandWaiters.set(receipt.requestId, (outcome) => {
        clearTimeout(timeout);
        resolve(outcome);
      });
    });
  }

  async submitCredential(secret: CredentialSubmission): Promise<CommandReceipt | undefined> {
    if (this.closed || this.state.lifecycle !== "ready") return undefined;
    this.publish({ ...this.state, commandError: undefined });
    try {
      return await this.transport.submitCredential(secret);
    } catch (error) {
      const message = error instanceof Error ? error.message : "The host rejected the credential";
      this.publish({ ...this.state, commandError: message });
      return undefined;
    }
  }

  applyFrame(frame: ClientFrame): void {
    if (this.closed || this.state.lifecycle === "failed") return;
    try {
      const incoming = revisionValue(frame.revision);
      const current = this.state.transportRevision
        ? revisionValue(this.state.transportRevision)
        : undefined;

      const baseline = frame.kind === "snapshot" &&
        (frame.reason === "resynchronization" || (current === undefined && frame.reason === "initial"));
      if (current !== undefined && incoming <= current) return;
      if (current === undefined && !baseline) {
        if (frame.kind === "notice") this.observeGapNotice(frame);
        void this.requestResync();
        return;
      }
      if (current !== undefined && !baseline && incoming !== current + 1n) {
        if (frame.kind === "notice") this.observeGapNotice(frame);
        void this.requestResync();
        return;
      }

      if (frame.kind === "snapshot") {
        validateSnapshot(frame.snapshot);
        if (frame.snapshot.transportRevision !== frame.revision) {
          throw new Error("Snapshot and frame revisions do not match");
        }
        const recovering = this.state.lifecycle === "resyncing" && frame.reason !== "resynchronization";
        this.reconcileOptimisticPrompts(frame.snapshot, frame.reason === "resynchronization");
        this.publish({
          ...this.state,
          lifecycle: recovering ? "resyncing" : "ready",
          projection: frame.snapshot,
          transportRevision: frame.revision,
          commandError: undefined,
          optimisticPrompts: this.optimisticPromptList(),
        });
        return;
      }

      if (frame.kind === "active_session_delta") {
        const projection = this.state.projection;
        const active = projection?.activeSession;
        if (
          !projection
          || !active
          || projection.activeSessionId !== frame.sessionId
          || active.id !== frame.sessionId
          || frame.summary.id !== frame.sessionId
        ) {
          throw new Error("Active-session delta does not match the authoritative baseline");
        }
        const { start, deleteCount, items } = frame.transcript;
        if (
          !Number.isSafeInteger(start)
          || !Number.isSafeInteger(deleteCount)
          || start < 0
          || deleteCount < 0
          || start + deleteCount > active.transcript.length
        ) {
          throw new Error("Active-session delta exceeds the authoritative transcript");
        }
        const tailUpdate = start + deleteCount === active.transcript.length;
        const transcript = tailUpdate
          ? active.transcript as TranscriptItem[]
          : [
              ...active.transcript.slice(0, start),
              ...items,
              ...active.transcript.slice(start + deleteCount),
            ];
        if (tailUpdate) transcript.splice(start, deleteCount, ...items);
        if (transcript.length > 65_536) {
          throw new Error("Active-session delta exceeds the transcript row limit");
        }
        const summaryIndex = projection.sessions.findIndex((session) => session.id === frame.sessionId);
        if (summaryIndex < 0) {
          throw new Error("Active-session delta has no matching session summary");
        }
        const sessions = [...projection.sessions];
        sessions[summaryIndex] = frame.summary;
        const attempt = frame.attempt ?? active.attempt;
        const nextProjection: ClientSnapshot = {
          ...projection,
          transportRevision: frame.revision,
          sessions,
          activeSession: {
            ...active,
            revision: frame.sessionRevision,
            title: frame.summary.title,
            selectedModelId: frame.selectedModelId,
            transcript,
            attempt,
          },
          pendingPermission: frame.pendingPermission,
          activity: [
            {
              id: "replay",
              label: "Session projection",
              detail: `revision ${frame.sessionRevision}`,
              status: "complete",
            },
            {
              id: "attempt",
              label: "Provider turn",
              detail: attempt.kind.replaceAll("_", " "),
              status:
                attempt.kind === "streaming" || attempt.kind === "cancelling"
                  ? "active"
                  : attempt.kind === "failed"
                    ? "warning"
                    : "complete",
            },
          ],
        };
        this.reconcileOptimisticPrompts(nextProjection);
        this.publish({
          ...this.state,
          lifecycle: this.state.lifecycle === "resyncing" ? "resyncing" : "ready",
          projection: nextProjection,
          transportRevision: frame.revision,
          commandError: undefined,
          optimisticPrompts: this.optimisticPromptList(),
        });
        return;
      }

      this.publish({
        ...this.state,
        lifecycle: this.state.lifecycle === "resyncing"
          ? "resyncing"
          : this.state.projection ? "ready" : this.state.lifecycle,
        transportRevision: frame.revision,
        notice: { level: frame.level, code: frame.code, message: frame.message },
        commandError: frame.level === "error" ? frame.message : this.state.commandError,
      });
      this.settleCommandNotice(frame);
    } catch (error) {
      this.fail(error);
    }
  }

  async requestResync(): Promise<void> {
    if (this.closed) return;
    if (this.resync) return this.resync;
    this.publish({ ...this.state, lifecycle: "resyncing" });
    this.resync = (async () => {
      try {
        const baseline = await this.transport.snapshot(this.state.transportRevision);
        if (this.closed) return;
        validateSnapshot(baseline);
        const currentRevision = this.state.transportRevision
          ? revisionValue(this.state.transportRevision)
          : undefined;
        if (currentRevision !== undefined && revisionValue(baseline.transportRevision) < currentRevision) {
          return;
        }
        this.reconcileOptimisticPrompts(baseline, true);
        this.publish({
          lifecycle: "ready",
          projection: baseline,
          transportRevision: baseline.transportRevision,
          commandError: this.state.commandError,
          notice: {
            level: "info",
            code: "projection_resynchronized",
            message: "The local view was repaired from the durable host snapshot.",
          },
          optimisticPrompts: this.optimisticPromptList(),
        });
      } catch (error) {
        this.fail(error);
      } finally {
        this.resync = undefined;
      }
    })();
    return this.resync;
  }

  private fail(error: unknown): void {
    const message = error instanceof Error ? error.message : "The GUI client could not start";
    this.commandWaiters.forEach((waiter) => waiter("unknown"));
    this.commandWaiters.clear();
    this.commandSettlements.clear();
    this.optimisticPrompts.clear();
    this.publish({ ...this.state, lifecycle: "failed", commandError: message, optimisticPrompts: [] });
  }

  private observeGapNotice(frame: Extract<ClientFrame, { kind: "notice" }>): void {
    this.publish({
      ...this.state,
      notice: { level: frame.level, code: frame.code, message: frame.message },
      commandError: frame.level === "error" ? frame.message : this.state.commandError,
    });
    this.settleCommandNotice(frame);
  }

  private settleCommandNotice(frame: Extract<ClientFrame, { kind: "notice" }>): void {
    const committed = frame.code === "command_committed" || frame.code === "authentication_completed";
    const rejected = frame.level === "error" || frame.code === "authentication_cancelled";
    if (!frame.requestId || (!committed && !rejected)) return;
    if (!committed && this.optimisticPrompts.delete(frame.requestId)) {
      this.publish({ ...this.state, optimisticPrompts: this.optimisticPromptList() });
    }
    const waiter = this.commandWaiters.get(frame.requestId);
    if (waiter) {
      this.commandWaiters.delete(frame.requestId);
      waiter(committed ? "committed" : "rejected");
      return;
    }
    if (this.commandSettlements.size >= 64) {
      const oldest = this.commandSettlements.keys().next().value as string | undefined;
      if (oldest) this.commandSettlements.delete(oldest);
    }
    this.commandSettlements.set(frame.requestId, committed);
  }

  private trackOptimisticPrompt(
    requestId: string,
    prompt: Omit<TrackedOptimisticPrompt, "requestId">,
  ): void {
    const tracked = { requestId, ...prompt };
    this.optimisticPrompts.set(requestId, tracked);
    this.reconcileOptimisticPrompts(this.state.projection);
    this.publish({ ...this.state, optimisticPrompts: this.optimisticPromptList() });
  }

  private reconcileOptimisticPrompts(snapshot?: ClientSnapshot, replace = false): void {
    for (const [requestId, prompt] of this.optimisticPrompts) {
      const observed = snapshot?.activeSession?.id === prompt.sessionId
        && snapshot.activeSession.transcript.some((item) => (
          item.kind === "message"
          && item.role === "user"
          && item.content === prompt.content
          && !prompt.baselineUserMessageIds.has(item.id)
        ));
      if (observed || replace) this.optimisticPrompts.delete(requestId);
    }
  }

  private optimisticPromptList(): readonly OptimisticPrompt[] {
    return [...this.optimisticPrompts.values()].map(({ requestId, sessionId, content }) => ({
      requestId,
      sessionId,
      content,
    }));
  }

  private publish(next: ClientStoreSnapshot): void {
    this.state = next;
    this.listeners.forEach((listener) => listener());
  }
}

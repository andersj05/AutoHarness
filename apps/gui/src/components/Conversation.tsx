import { useEffect, useLayoutEffect, useMemo, useRef, useState, type Dispatch, type SetStateAction } from "react";
import type {
  ActiveSessionProjection,
  CatalogProjection,
  CommandOutcome,
  ComposerSubmitBehavior,
  ConnectionState,
  ModelDescriptor,
  TextMessage,
  TimestampStyle,
  TranscriptItem,
} from "../protocol";
import type { OptimisticPrompt } from "../store/clientStore";
import { Composer } from "./Composer";
import { Icon } from "./Icon";
import { Button, Callout, ToolCard } from "./primitives";
import { MessageContent } from "./MessageContent";
import { ActionMenu } from "./primitives/ActionMenu";
import { VirtualTranscript } from "./VirtualTranscript";

interface ConversationProps {
  catalog: CatalogProjection;
  connection: ConnectionState;
  draft: string;
  model?: ModelDescriptor;
  runtimeMode: "native" | "fixture";
  session?: ActiveSessionProjection;
  interactionBlocked?: boolean;
  optimisticPrompts?: readonly OptimisticPrompt[];
  searchRequest?: number;
  submissionBehavior: ComposerSubmitBehavior;
  timestampStyle: TimestampStyle;
  onCancel: (attemptId: string) => void;
  onDraftChange: Dispatch<SetStateAction<string>>;
  onOpenCredential: () => void;
  onOpenInspector: () => void;
  onOpenModelPicker: () => void;
  onOpenNavigation: () => void;
  onRefresh: () => void;
  onRetry: (attemptId: string) => void;
  onExport: () => Promise<CommandOutcome>;
  onSubmit: (prompt: string) => Promise<CommandOutcome>;
}

function transcriptSearchText(item: TranscriptItem): string {
  return item.kind === "message"
    ? item.content
    : [item.name, item.summary, item.resource, item.detail, item.failure?.code, item.failure?.message].filter(Boolean).join("\n");
}

function transcriptPlainText(items: readonly TranscriptItem[]): string {
  return items.map((item) => {
    if (item.kind === "message") return `${item.role === "user" ? "You" : "AutoHarness"}:\n${item.content}`;
    const detail = item.failure ? `${item.failure.code}: ${item.failure.message}` : item.detail;
    return [`Tool ${item.name} [${item.status}]`, `Resource: ${item.resource}`, item.summary, detail].filter(Boolean).join("\n");
  }).join("\n\n");
}

function messageTimestamp(date: Date, style: TimestampStyle): string {
  if (style === "absolute") {
    return new Intl.DateTimeFormat(undefined, { dateStyle: "medium", timeStyle: "short" }).format(date);
  }
  const deltaSeconds = Math.round((date.getTime() - Date.now()) / 1000);
  const absoluteSeconds = Math.abs(deltaSeconds);
  const [value, unit] = absoluteSeconds < 60
    ? [deltaSeconds, "second" as const]
    : absoluteSeconds < 3_600
      ? [Math.round(deltaSeconds / 60), "minute" as const]
      : absoluteSeconds < 86_400
        ? [Math.round(deltaSeconds / 3_600), "hour" as const]
        : [Math.round(deltaSeconds / 86_400), "day" as const];
  return new Intl.RelativeTimeFormat(undefined, { numeric: "auto" }).format(value, unit);
}

function MessageTurn({ highlight, message, optimistic = false, timestampStyle }: { highlight?: string; message: TextMessage; optimistic?: boolean; timestampStyle: TimestampStyle }) {
  const [copied, setCopied] = useState(false);
  const [copyFailed, setCopyFailed] = useState(false);
  const timestamp = message.createdAt ? new Date(message.createdAt) : undefined;
  const hasValidTimestamp = timestamp && Number.isFinite(timestamp.getTime());
  return (
    <article className="messageTurn" data-role={message.role}>
      <header>
        <span className={message.role === "agent" ? "agentAvatar" : "userAvatar"} aria-hidden="true">
          {message.role === "agent" ? <Icon name="spark" size={15} /> : "Y"}
        </span>
        <div>
          <strong>{message.role === "agent" ? "AutoHarness" : "You"}</strong>
          {timestampStyle !== "hidden" && hasValidTimestamp && timestamp && message.createdAt ? (
            <time dateTime={message.createdAt} title={new Intl.DateTimeFormat(undefined, { dateStyle: "full", timeStyle: "long" }).format(timestamp)}>
              {messageTimestamp(timestamp, timestampStyle)}
            </time>
          ) : null}
        </div>
        {optimistic ? <span className="optimisticLabel">Sending…</span> : null}
      </header>
      <div className="messageContent">
        <MessageContent text={message.content} highlight={highlight} plain={message.role === "user"} />
        {message.streaming ? (
          <span aria-label="Response streaming" className="streamTrace" role="status">
            <i /><i /><i /><i />
          </span>
        ) : null}
      </div>
      {message.role === "agent" && !message.streaming ? <div className="messageActions">
        <button aria-label="Copy response" className="quietIconButton" title="Copy response" onClick={() => {
          void (async () => {
            try { await navigator.clipboard.writeText(message.content); setCopied(true); setCopyFailed(false); }
            catch { setCopyFailed(true); }
          })();
        }} type="button"><Icon name={copied ? "check" : "copy"} size={14} /></button>
        <span role="status">{copyFailed ? "Could not copy" : copied ? "Copied" : ""}</span>
      </div> : null}
    </article>
  );
}

export function Conversation({
  catalog,
  connection,
  draft,
  interactionBlocked = false,
  optimisticPrompts = [],
  searchRequest = 0,
  submissionBehavior,
  timestampStyle,
  model,
  session,
  onCancel,
  onDraftChange,
  onOpenCredential,
  onOpenInspector,
  onOpenModelPicker,
  onOpenNavigation,
  onRefresh,
  onRetry,
  onExport,
  onSubmit,
}: ConversationProps) {
  const scrollRef = useRef<HTMLDivElement>(null);
  const followTailRef = useRef(true);
  const previousSessionRef = useRef<string>();
  const searchInputRef = useRef<HTMLInputElement>(null);
  const [searchOpen, setSearchOpen] = useState(false);
  const [searchQuery, setSearchQuery] = useState("");
  const [matchCursor, setMatchCursor] = useState(0);
  const [copyState, setCopyState] = useState<"idle" | "copied" | "failed">("idle");
  const [exporting, setExporting] = useState(false);
  const [awayFromTail, setAwayFromTail] = useState(false);
  const offline = connection.kind === "offline";
  const credentialRequired = catalog.status === "credential_required" || connection.kind === "credential_required";
  const attempt = session?.attempt ?? { kind: "idle" as const };
  const transcript = useMemo<readonly TranscriptItem[]>(() => {
    if (!session) return [];
    const pending = optimisticPrompts
      .filter((prompt) => prompt.sessionId === session.id)
      .map((prompt): TextMessage => ({
        kind: "message",
        id: `optimistic:${prompt.requestId}`,
        role: "user",
        content: prompt.content,
      }));
    return pending.length > 0 ? [...session.transcript, ...pending] : session.transcript;
  }, [optimisticPrompts, session]);
  const lastItem = transcript[transcript.length - 1];
  const tailVersion = lastItem?.kind === "message"
    ? `${lastItem.id}:${lastItem.content.length}:${String(lastItem.streaming)}`
    : lastItem
      ? `${lastItem.id}:${lastItem.status}:${lastItem.detail ?? ""}`
      : "empty";
  const normalizedSearch = searchQuery.trim().toLocaleLowerCase();
  const matches = useMemo(() => {
    if (!normalizedSearch) return [];
    const result: number[] = [];
    transcript.forEach((item, index) => {
      if (transcriptSearchText(item).toLocaleLowerCase().includes(normalizedSearch)) result.push(index);
    });
    return result;
  }, [normalizedSearch, session?.revision, transcript]);
  const activeMatch = matches.length > 0 ? matches[Math.min(matchCursor, matches.length - 1)] : undefined;

  useEffect(() => {
    setSearchOpen(false);
    setSearchQuery("");
    setMatchCursor(0);
    setCopyState("idle");
    setAwayFromTail(false);
  }, [session?.id]);

  useEffect(() => {
    setMatchCursor(0);
  }, [normalizedSearch]);

  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (interactionBlocked || !(event.ctrlKey || event.metaKey) || event.key.toLocaleLowerCase() !== "f") return;
      event.preventDefault();
      setSearchOpen(true);
      queueMicrotask(() => searchInputRef.current?.focus());
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [interactionBlocked]);

  useEffect(() => {
    if (searchRequest <= 0 || interactionBlocked) return;
    setSearchOpen(true);
    queueMicrotask(() => searchInputRef.current?.focus());
  }, [interactionBlocked, searchRequest]);

  const moveMatch = (direction: 1 | -1) => {
    if (matches.length === 0) return;
    setMatchCursor((current) => (current + direction + matches.length) % matches.length);
  };

  const copyTranscript = async () => {
    if (!session || !navigator.clipboard?.writeText) {
      setCopyState("failed");
      return;
    }
    try {
      await navigator.clipboard.writeText(transcriptPlainText(session.transcript));
      setCopyState("copied");
    } catch {
      setCopyState("failed");
    }
  };

  useLayoutEffect(() => {
    const container = scrollRef.current;
    if (!container) return;
    const sessionChanged = previousSessionRef.current !== session?.id;
    previousSessionRef.current = session?.id;
    if (sessionChanged) followTailRef.current = true;
    if (followTailRef.current) container.scrollTop = container.scrollHeight;
  }, [attempt.kind, session?.id, tailVersion]);
  const disabledReason = credentialRequired
    ? "Connect the active provider before sending."
    : offline
      ? "Reconnect a provider to send. Your draft stays local."
      : catalog.status === "empty"
      ? "Refresh the model catalog before sending."
      : !model
        ? "Choose a compatible model before sending."
        : !model.selectable
          ? "The selected model is currently unavailable. Choose another model."
        : undefined;

  return (
    <main className="conversationWorkspace" id="main-content" tabIndex={-1}>
      <header className="conversationHeader">
        <div className="headerIdentity">
          <button aria-label="Open navigation" className="iconButton mobileMenu" onClick={onOpenNavigation} type="button">
            <Icon name="menu" />
          </button>
          <div>
            <h1>{session?.title ?? "Chat"}</h1>
          </div>
        </div>
        <div className="headerActions">
          <button aria-label="Search transcript" className="iconButton" onClick={() => { setSearchOpen(true); queueMicrotask(() => searchInputRef.current?.focus()); }} title="Search transcript (Ctrl F)" type="button">
            <Icon name="search" />
          </button>
          <button aria-label="Open context inspector" className="iconButton" onClick={onOpenInspector} type="button">
            <Icon name="panel-right" />
          </button>
          <ActionMenu label="Session actions" blocked={interactionBlocked} items={[
            { id: "copy", label: "Copy transcript", icon: "copy", disabled: transcript.length === 0 },
            { id: "export", label: exporting ? "Exporting…" : "Export transcript", icon: "download", disabled: exporting || transcript.length === 0 },
          ]} onAction={(id) => {
            if (id === "copy") void copyTranscript();
            else { setExporting(true); void onExport().finally(() => setExporting(false)); }
          }} />
        </div>
      </header>

      {searchOpen ? (
        <div className="transcriptSearch" role="search">
          <Icon name="search" size={15} />
          <label><span className="srOnly">Find in transcript</span><input
            aria-label="Find in transcript"
            onChange={(event) => setSearchQuery(event.target.value.slice(0, 256))}
            onKeyDown={(event) => {
              if (event.key === "Escape") {
                event.preventDefault();
                setSearchOpen(false);
              } else if (event.key === "Enter") {
                event.preventDefault();
                moveMatch(event.shiftKey ? -1 : 1);
              }
            }}
            placeholder="Find messages, tools, paths…"
            ref={searchInputRef}
            type="search"
            value={searchQuery}
          /></label>
          <span aria-live="polite">{normalizedSearch ? `${matches.length === 0 ? 0 : matchCursor + 1} / ${matches.length}` : "Type to search"}</span>
          <button aria-label="Previous match" className="quietIconButton" disabled={matches.length === 0} onClick={() => moveMatch(-1)} type="button"><span aria-hidden="true">↑</span></button>
          <button aria-label="Next match" className="quietIconButton" disabled={matches.length === 0} onClick={() => moveMatch(1)} type="button"><span aria-hidden="true">↓</span></button>
          <button aria-label="Close transcript search" className="quietIconButton" onClick={() => setSearchOpen(false)} type="button"><Icon name="close" size={14} /></button>
        </div>
      ) : null}

      <div
        className="conversationScroll"
        onScroll={(event) => {
          const container = event.currentTarget;
          followTailRef.current = container.scrollHeight - container.scrollTop - container.clientHeight <= 96;
          setAwayFromTail(!followTailRef.current);
        }}
        ref={scrollRef}
      >
        <div className="conversationColumn">
          {offline && !credentialRequired ? (
            <Callout
              action={<Button onClick={onRefresh}>Try reconnecting</Button>}
              detail={connection.reason}
              icon="warning"
              intent="warning"
              title="You’re offline"
            />
          ) : null}
          {credentialRequired ? (
            <Callout
              action={<Button onClick={onOpenCredential}>Enter credential</Button>}
              detail={connection.kind === "credential_required" ? connection.reason : "Add an API key to connect your provider."}
              icon="warning"
              intent="warning"
              title="Connect the active provider"
            />
          ) : null}
          {catalog.status === "empty" && !credentialRequired ? (
            <Callout
              action={<Button onClick={onRefresh}>Refresh models</Button>}
              detail="The provider returned no compatible chat models. Existing sessions remain available."
              icon="model"
              intent="warning"
              title="No compatible models"
            />
          ) : null}
          {catalog.status === "failed" ? (
            <Callout
              action={<Button onClick={onRefresh}>Retry catalog</Button>}
              detail={catalog.safeError ?? "Model discovery did not complete."}
              icon="refresh"
              intent="danger"
              title="Catalog refresh failed"
            />
          ) : null}

          {transcript.length ? (
            <VirtualTranscript
              activeIndex={activeMatch}
              items={transcript}
              renderItem={(item, index) =>
                item.kind === "message" ? <MessageTurn highlight={index === activeMatch ? searchQuery.trim() : undefined} message={item} optimistic={item.id.startsWith("optimistic:")} timestampStyle={timestampStyle} /> : (
                  <ToolCard forceOpen={index === activeMatch} name={item.name} resource={item.resource} status={item.status} summary={item.summary}>
                    {item.failure ? (
                      <>
                        <div><span>Failure</span><p>{item.failure.message}</p></div>
                        <div><span>Code</span><code>{item.failure.code}</code></div>
                      </>
                    ) : item.detail ? <div><span>Result</span><p>{item.detail}</p></div> : null}
                  </ToolCard>
                )}
              scrollRef={scrollRef}
              sessionId={session?.id}
            />
          ) : (
            <section aria-label="Conversation transcript" className="transcript" tabIndex={-1}>
              <div className="emptyConversation">
                <h2>What would you like to work on?</h2>
              </div>
            </section>
          )}

          <div aria-live="polite" className="copyAnnouncer">{copyState === "copied" ? "Transcript copied to the clipboard." : copyState === "failed" ? "The transcript could not be copied." : ""}</div>

          {attempt.kind === "failed" ? (
            <section className="attemptFailure" role="alert">
              <span className="calloutIcon"><Icon name="warning" /></span>
              <div><strong>Response interrupted</strong><p>{attempt.message}</p><code>{attempt.code}</code></div>
              {attempt.retryable ? <Button icon="refresh" onClick={() => onRetry(attempt.id)}>Retry</Button> : null}
            </section>
          ) : null}
          {attempt.kind === "cancelled" ? (
            <section className="cancelledState" role="status">
              <span>Generation stopped. The partial response remains in this session.</span>
              <Button icon="refresh" onClick={() => onRetry(attempt.id)} size="small" variant="quiet">Retry turn</Button>
            </section>
          ) : null}

          {awayFromTail ? <button className="jumpToLatest" onClick={() => {
            followTailRef.current = true;
            if (scrollRef.current) scrollRef.current.scrollTop = scrollRef.current.scrollHeight;
            setAwayFromTail(false);
          }} type="button"><Icon name="arrow-up" size={14} />Jump to latest</button> : null}
          <Composer
            attempt={attempt}
            draft={draft}
            disabledReason={disabledReason}
            key={session?.id ?? "no-session"}
            model={model}
            onCancel={onCancel}
            onDraftChange={onDraftChange}
            onOpenModelPicker={onOpenModelPicker}
            onSubmit={onSubmit}
            submissionBehavior={submissionBehavior}
          />
        </div>
      </div>
    </main>
  );
}

import { useLayoutEffect, useRef, type Dispatch, type SetStateAction } from "react";
import type {
  ActiveSessionProjection,
  CatalogProjection,
  CommandOutcome,
  ConnectionState,
  ModelDescriptor,
  TextMessage,
} from "../protocol";
import { Composer } from "./Composer";
import { Icon } from "./Icon";
import { Button, Callout, ToolCard } from "./primitives";

interface ConversationProps {
  catalog: CatalogProjection;
  connection: ConnectionState;
  draft: string;
  model?: ModelDescriptor;
  runtimeMode: "native" | "fixture";
  session?: ActiveSessionProjection;
  onCancel: (attemptId: string) => void;
  onDraftChange: Dispatch<SetStateAction<string>>;
  onOpenCredential: () => void;
  onOpenInspector: () => void;
  onOpenModelPicker: () => void;
  onOpenNavigation: () => void;
  onRefresh: () => void;
  onRetry: (attemptId: string) => void;
  onSubmit: (prompt: string) => Promise<CommandOutcome>;
}

function MessageTurn({ message }: { message: TextMessage }) {
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
          {hasValidTimestamp && timestamp && message.createdAt ? (
            <time dateTime={message.createdAt}>
              {new Intl.DateTimeFormat(undefined, { hour: "numeric", minute: "2-digit" }).format(timestamp)}
            </time>
          ) : null}
        </div>
        {message.role === "agent" ? <span className="agentLabel">agent</span> : null}
      </header>
      <div className="messageContent">
        {message.content.split("\n").map((line, index) =>
          line.length > 0 ? <p key={`${message.id}-${index}`}>{line}</p> : <span aria-hidden="true" className="messageSpacer" key={`${message.id}-${index}`} />,
        )}
        {message.streaming ? (
          <span aria-label="Response streaming" className="streamTrace" role="status">
            <i /><i /><i /><i />
          </span>
        ) : null}
      </div>
    </article>
  );
}

export function Conversation({
  catalog,
  connection,
  draft,
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
  onSubmit,
  runtimeMode,
}: ConversationProps) {
  const scrollRef = useRef<HTMLDivElement>(null);
  const followTailRef = useRef(true);
  const previousSessionRef = useRef<string>();
  const offline = connection.kind === "offline";
  const credentialRequired = catalog.status === "credential_required" || connection.kind === "credential_required";
  const attempt = session?.attempt ?? { kind: "idle" as const };
  const lastItem = session?.transcript[session.transcript.length - 1];
  const tailVersion = lastItem?.kind === "message"
    ? `${lastItem.id}:${lastItem.content.length}:${String(lastItem.streaming)}`
    : lastItem
      ? `${lastItem.id}:${lastItem.status}:${lastItem.detail ?? ""}`
      : "empty";

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
    <main className="conversationWorkspace" id="main-content">
      <header className="conversationHeader">
        <div className="headerIdentity">
          <button aria-label="Open navigation" className="iconButton mobileMenu" onClick={onOpenNavigation} type="button">
            <Icon name="menu" />
          </button>
          <div>
            <div className="breadcrumb"><span>workspace</span><i>/</i><span>chat</span></div>
            <h1>{session?.title ?? "Chat"}</h1>
          </div>
        </div>
        <div className="headerActions">
          {runtimeMode === "fixture" ? (
            <span className="fixtureBanner" role="status" title="Browser fixture - simulated state only">
              <Icon name="warning" size={13} />
              <span aria-hidden="true">Fixture</span>
              <span className="srOnly">Browser fixture - simulated state only</span>
            </span>
          ) : null}
          <span className="connectionChip" data-state={connection.kind}>
            <span />
            {connection.kind === "online" ? "connected" : connection.kind === "connecting" ? "connecting" : connection.kind === "credential_required" ? "credential" : "offline"}
          </span>
          <button aria-label={`Change model, current ${model?.displayName ?? "none"}`} className="headerModelButton" onClick={onOpenModelPicker} type="button">
            <Icon name="model" size={15} />
            <span>{model?.displayName ?? "Select model"}</span>
            <span className="tinyChevron">⌄</span>
          </button>
          <button aria-label="Open context inspector" className="iconButton" onClick={onOpenInspector} type="button">
            <Icon name="panel-right" />
          </button>
        </div>
      </header>

      <div
        className="conversationScroll"
        onScroll={(event) => {
          const container = event.currentTarget;
          followTailRef.current = container.scrollHeight - container.scrollTop - container.clientHeight <= 96;
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
              title={runtimeMode === "fixture" ? "Fixture provider offline" : "Working offline from durable replay"}
            />
          ) : null}
          {credentialRequired ? (
            <Callout
              action={<Button onClick={onOpenCredential}>Enter credential</Button>}
              detail={connection.kind === "credential_required" ? connection.reason : "The active provider needs a credential. It will cross a dedicated one-way secret boundary."}
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

          <section aria-label="Conversation transcript" className="transcript" tabIndex={-1}>
            {session?.transcript.length ? (
              session.transcript.map((item) =>
                item.kind === "message" ? <MessageTurn key={item.id} message={item} /> : (
                  <ToolCard key={item.id} name={item.name} resource={item.resource} status={item.status} summary={item.summary}>
                    {item.failure ? (
                      <>
                        <div><span>Failure</span><p>{item.failure.message}</p></div>
                        <div><span>Code</span><code>{item.failure.code}</code></div>
                      </>
                    ) : item.detail ? <div><span>Result</span><p>{item.detail}</p></div> : null}
                  </ToolCard>
                ),
              )
            ) : (
              <div className="emptyConversation">
                <span className="emptyConversationIcon"><Icon name="spark" size={25} /></span>
                <p className="eyebrow">{runtimeMode === "fixture" ? "Fixture conversation" : "New durable session"}</p>
                <h2>What should we build?</h2>
                <p>{runtimeMode === "fixture" ? "Prompts and responses are simulated for visual review and are not persisted." : "Prompts and responses become replayable events. Tools still require exact capability authority."}</p>
              </div>
            )}
          </section>

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
            runtimeMode={runtimeMode}
          />
        </div>
      </div>
    </main>
  );
}

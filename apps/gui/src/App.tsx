import { useEffect, useMemo, useRef, useState, type SetStateAction } from "react";
import { AppRail, type RouteId } from "./components/AppRail";
import { ContextInspector } from "./components/ContextInspector";
import { Conversation } from "./components/Conversation";
import { CredentialDialog } from "./components/CredentialDialog";
import { Icon } from "./components/Icon";
import { ModelPicker } from "./components/ModelPicker";
import { PermissionDialog } from "./components/PermissionDialog";
import { SessionsWorkspace, SimpleWorkspace } from "./components/RouteWorkspaces";
import { useClientStore } from "./store/react";
import type { ClientStore } from "./store/clientStore";

interface AppProps {
  store: ClientStore;
}

function mediaMatches(query: string): boolean {
  return typeof window.matchMedia === "function" && window.matchMedia(query).matches;
}

function useMediaQuery(query: string): boolean {
  const [matches, setMatches] = useState(() => mediaMatches(query));
  useEffect(() => {
    if (typeof window.matchMedia !== "function") return;
    const media = window.matchMedia(query);
    const update = () => setMatches(media.matches);
    update();
    media.addEventListener("change", update);
    return () => media.removeEventListener("change", update);
  }, [query]);
  return matches;
}

export function App({ store }: AppProps) {
  const client = useClientStore(store);
  const [route, setRoute] = useState<RouteId>("chat");
  const [railCollapsed, setRailCollapsed] = useState(false);
  const [mobileRailOpen, setMobileRailOpen] = useState(false);
  const [inspectorOpen, setInspectorOpen] = useState(() => !mediaMatches("(max-width: 1180px)"));
  const [modelPickerOpen, setModelPickerOpen] = useState(false);
  const [credentialOpen, setCredentialOpen] = useState(false);
  const [highContrast, setHighContrast] = useState(false);
  const [reduceMotion, setReduceMotion] = useState(false);
  const [permissionAnswering, setPermissionAnswering] = useState(false);
  const [sessionDrafts, setSessionDrafts] = useState<Record<string, string>>({});
  const mobileViewport = useMediaQuery("(max-width: 680px)");
  const shellRef = useRef<HTMLDivElement>(null);
  const workspaceRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    void store.start();
  }, [store]);

  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      const newSessionShortcut = (event.ctrlKey || event.metaKey) && event.key.toLocaleLowerCase() === "n";
      if (!newSessionShortcut) return;
      event.preventDefault();
      if (
        client.lifecycle !== "ready" ||
        event.repeat ||
        client.projection?.pendingPermission ||
        modelPickerOpen ||
        credentialOpen ||
        mobileRailOpen
      ) return;
      void store.dispatch({ type: "create_session" });
      setRoute("chat");
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [client.lifecycle, client.projection?.pendingPermission, credentialOpen, mobileRailOpen, modelPickerOpen, store]);

  const projection = client.projection;
  const activeSession = projection?.activeSession;
  const activeSessionId = activeSession?.id;
  const pendingPermissionIdentity = projection?.pendingPermission
    ? `${projection.pendingPermission.sessionId}:${projection.pendingPermission.id}`
    : undefined;
  const blockingDialogOpen = Boolean(projection?.pendingPermission) || modelPickerOpen || credentialOpen;
  const activeDraft = activeSessionId ? sessionDrafts[activeSessionId] ?? "" : "";
  const setActiveDraft = (next: SetStateAction<string>) => {
    if (!activeSessionId) return;
    setSessionDrafts((current) => {
      const currentDraft = current[activeSessionId] ?? "";
      const value = typeof next === "function" ? next(currentDraft) : next;
      if (value === currentDraft) return current;
      return { ...current, [activeSessionId]: value };
    });
  };
  const activeModel = useMemo(
    () => projection?.catalog.models.find((model) => model.id === activeSession?.selectedModelId),
    [activeSession?.selectedModelId, projection?.catalog.models],
  );

  useEffect(() => {
    if (pendingPermissionIdentity) {
      setModelPickerOpen(false);
      setCredentialOpen(false);
      setMobileRailOpen(false);
    }
    setPermissionAnswering(false);
  }, [pendingPermissionIdentity]);

  useEffect(() => {
    if (blockingDialogOpen) {
      shellRef.current?.setAttribute("inert", "");
      shellRef.current?.setAttribute("aria-hidden", "true");
    } else {
      shellRef.current?.removeAttribute("inert");
      shellRef.current?.removeAttribute("aria-hidden");
    }
  }, [blockingDialogOpen]);

  useEffect(() => {
    if (mobileViewport && mobileRailOpen) {
      workspaceRef.current?.setAttribute("inert", "");
      workspaceRef.current?.setAttribute("aria-hidden", "true");
    } else {
      workspaceRef.current?.removeAttribute("inert");
      workspaceRef.current?.removeAttribute("aria-hidden");
    }
  }, [mobileRailOpen, mobileViewport]);

  if (client.lifecycle === "failed") {
    return (
      <main className="fatalSurface">
        <span className="fatalIcon"><Icon name="warning" size={25} /></span>
        <p className="eyebrow">Desktop client unavailable</p>
        <h1>AutoHarness could not open the local runtime</h1>
        <p>{client.commandError ?? "The renderer did not receive an authoritative startup snapshot."}</p>
        <button className="button primary" onClick={() => void store.requestResync()} type="button"><Icon name="refresh" size={16} /> Try again</button>
      </main>
    );
  }

  if (client.lifecycle === "resyncing") {
    return (
      <main aria-busy="true" className="bootSurface">
        <div className="bootMark"><span /><span /><span /></div>
        <p className="eyebrow">Local runtime</p>
        <h1>Repairing the desktop view</h1>
        <div className="bootTrace"><i /><i /><i /><i /><i /></div>
        <p>Waiting for a fresh authoritative snapshot before commands are enabled.</p>
      </main>
    );
  }

  if (!projection) {
    return (
      <main aria-busy="true" className="bootSurface">
        <div className="bootMark"><span /><span /><span /></div>
        <p className="eyebrow">Local runtime</p>
        <h1>Opening your workspace</h1>
        <div className="bootTrace"><i /><i /><i /><i /><i /></div>
        <p>Replaying the durable session and restoring provider state.</p>
      </main>
    );
  }

  const openSession = (sessionId: string) => {
    if (sessionId !== projection.activeSessionId) void store.dispatch({ type: "open_session", sessionId });
    setRoute("chat");
  };

  const routeWorkspace =
    route === "chat" ? (
      <Conversation
        catalog={projection.catalog}
        connection={projection.connection}
        draft={activeDraft}
        model={activeModel}
        onCancel={(attemptId) => {
          if (activeSession) void store.dispatch({ type: "cancel_attempt", sessionId: activeSession.id, attemptId });
        }}
        onDraftChange={setActiveDraft}
        onOpenCredential={() => setCredentialOpen(true)}
        onOpenInspector={() => setInspectorOpen(true)}
        onOpenModelPicker={() => setModelPickerOpen(true)}
        onOpenNavigation={() => setMobileRailOpen(true)}
        onRefresh={() => void store.dispatch({ type: "refresh_catalog" })}
        onRetry={(attemptId) => {
          if (activeSession) void store.dispatch({ type: "retry_attempt", sessionId: activeSession.id, attemptId });
        }}
        onSubmit={async (prompt) => {
          if (!activeSession) return "rejected";
          return store.dispatchAndWait({ type: "submit_prompt", sessionId: activeSession.id, prompt });
        }}
        runtimeMode={projection.runtimeMode}
        session={activeSession}
      />
    ) : route === "sessions" ? (
      <SessionsWorkspace onOpen={openSession} onOpenNavigation={() => setMobileRailOpen(true)} snapshot={projection} />
    ) : (
      <SimpleWorkspace
        highContrast={highContrast}
        onHighContrast={setHighContrast}
        onOpenNavigation={() => setMobileRailOpen(true)}
        onReduceMotion={setReduceMotion}
        reduceMotion={reduceMotion}
        route={route}
      />
    );

  return (
    <div className="app" data-high-contrast={highContrast} data-reduce-motion={reduceMotion}>
      <a aria-hidden={blockingDialogOpen || (mobileViewport && mobileRailOpen) ? true : undefined} className="skipLink" href="#main-content" tabIndex={blockingDialogOpen || (mobileViewport && mobileRailOpen) ? -1 : undefined}>Skip to main content</a>
      <div className="appShell" ref={shellRef}>
        <div className="ambient ambientOne" />
        <div className="ambient ambientTwo" />
        {mobileRailOpen ? <button aria-label="Dismiss navigation drawer" className="mobileScrim" onClick={() => setMobileRailOpen(false)} type="button" /> : null}
        <AppRail
        activeRoute={route}
        activeSessionId={projection.activeSessionId}
        collapsed={railCollapsed}
        mobileViewport={mobileViewport}
        mobileOpen={mobileRailOpen}
        onCloseMobile={() => setMobileRailOpen(false)}
        onCreateSession={() => {
          void store.dispatch({ type: "create_session" });
          setRoute("chat");
        }}
        onOpenSession={openSession}
        onRoute={setRoute}
        onToggleCollapsed={() => setRailCollapsed((value) => !value)}
        runtimeMode={projection.runtimeMode}
        sessions={projection.sessions}
        />
        <div className="workspaceSurface" ref={workspaceRef}>
          {routeWorkspace}
          {route === "chat" && inspectorOpen ? (
            <ContextInspector
            activity={projection.activity}
            connection={projection.connection}
            mobileOpen={inspectorOpen}
            model={activeModel}
            onClose={() => setInspectorOpen(false)}
            runtimeMode={projection.runtimeMode}
            session={activeSession}
            />
          ) : null}
        </div>
      </div>

      {modelPickerOpen && !projection.pendingPermission ? (
        <ModelPicker
        models={projection.catalog.models}
        onClose={() => setModelPickerOpen(false)}
        onRefresh={() => void store.dispatch({ type: "refresh_catalog" })}
        onSelect={(modelId) => {
          if (activeSession) void store.dispatch({ type: "select_model", sessionId: activeSession.id, modelId });
        }}
        selectedModelId={activeSession?.selectedModelId}
        />
      ) : null}
      {credentialOpen && !projection.pendingPermission && projection.connectionId ? (
        <CredentialDialog
          connectionId={projection.connectionId}
          onClose={() => setCredentialOpen(false)}
          onSubmit={async (connectionId, credential) => {
            const receipt = await store.submitCredential({ connectionId, credential });
            return Boolean(receipt);
          }}
          providerLabel={projection.connection.kind === "offline" ? "provider" : projection.connection.providerLabel}
        />
      ) : null}

      {projection.pendingPermission ? (
        <PermissionDialog
          busy={permissionAnswering}
          onAllow={() => {
            if (permissionAnswering) return;
            setPermissionAnswering(true);
            void store.dispatchAndWait({
              type: "answer_permission",
              sessionId: projection.pendingPermission!.sessionId,
              toolCallId: projection.pendingPermission!.id,
              decision: "allow_once",
            }).then((outcome) => { if (outcome !== "committed") setPermissionAnswering(false); });
          }}
          onDeny={() => {
            if (permissionAnswering) return;
            setPermissionAnswering(true);
            void store.dispatchAndWait({
              type: "answer_permission",
              sessionId: projection.pendingPermission!.sessionId,
              toolCallId: projection.pendingPermission!.id,
              decision: "deny",
            }).then((outcome) => { if (outcome !== "committed") setPermissionAnswering(false); });
          }}
          permission={projection.pendingPermission}
        />
      ) : null}

      <div aria-atomic="true" aria-live="polite" className="statusAnnouncer">
        {client.notice?.message}
      </div>
      {client.commandError || client.notice?.level === "error" ? (
        <div className="toast" data-intent="error" role="alert"><Icon name="warning" size={16} /><span>{client.commandError ?? client.notice?.message}</span></div>
      ) : null}
    </div>
  );
}

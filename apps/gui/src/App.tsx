import { useEffect, useMemo, useRef, useState, type CSSProperties, type SetStateAction } from "react";
import { AppRail, type RouteId } from "./components/AppRail";
import { ContextInspector } from "./components/ContextInspector";
import { Conversation } from "./components/Conversation";
import { CredentialDialog } from "./components/CredentialDialog";
import { Icon } from "./components/Icon";
import { ModelPicker } from "./components/ModelPicker";
import { PermissionDialog } from "./components/PermissionDialog";
import { ProvidersWorkspace } from "./components/ProvidersWorkspace";
import { MemoryWorkspace } from "./components/RouteWorkspaces";
import { SessionsWorkspace } from "./components/SessionsWorkspace";
import { SettingsWorkspace } from "./components/SettingsWorkspace";
import { Button, CommandPalette, SplitPane, type CommandItem } from "./components/primitives";
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
  const [railWidth, setRailWidth] = useState(248);
  const [mobileRailOpen, setMobileRailOpen] = useState(false);
  const [inspectorOpen, setInspectorOpen] = useState(() => !mediaMatches("(max-width: 1180px)"));
  const [modelPickerOpen, setModelPickerOpen] = useState(false);
  const [credentialOpen, setCredentialOpen] = useState(false);
  const [commandPaletteOpen, setCommandPaletteOpen] = useState(false);
  const [transcriptSearchRequest, setTranscriptSearchRequest] = useState(0);
  const [inspectorPercent, setInspectorPercent] = useState(72);
  const [answeringPermissionIdentity, setAnsweringPermissionIdentity] = useState<string>();
  const [sessionDrafts, setSessionDrafts] = useState<Record<string, string>>({});
  const mobileViewport = useMediaQuery("(max-width: 680px)");
  const compactInspectorViewport = useMediaQuery("(max-width: 1180px)");
  const systemDarkTheme = useMediaQuery("(prefers-color-scheme: dark)");
  const systemReducedMotion = useMediaQuery("(prefers-reduced-motion: reduce)");
  const shellRef = useRef<HTMLDivElement>(null);
  const workspaceRef = useRef<HTMLDivElement>(null);
  const previousRouteRef = useRef<RouteId>(route);

  useEffect(() => {
    void store.start();
  }, [store]);

  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      const newSessionShortcut = (event.ctrlKey || event.metaKey) && event.key.toLocaleLowerCase() === "n";
      const paletteShortcut = (event.ctrlKey || event.metaKey) && event.key.toLocaleLowerCase() === "k";
      const routeShortcut = event.altKey && !event.ctrlKey && !event.metaKey
        ? ({ "1": "chat", "2": "sessions", "3": "providers", "4": "memory", "5": "settings" } as const)[event.key]
        : undefined;
      if (!newSessionShortcut && !paletteShortcut && !routeShortcut) return;
      event.preventDefault();
      if (client.lifecycle !== "ready" || event.repeat || client.projection?.pendingPermission || modelPickerOpen || credentialOpen || mobileRailOpen) return;
      if (routeShortcut) {
        setCommandPaletteOpen(false);
        setRoute(routeShortcut);
      } else if (paletteShortcut) {
        setCommandPaletteOpen(true);
      } else if (!commandPaletteOpen) {
        void store.dispatch({ type: "create_session" });
        setRoute("chat");
      }
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [client.lifecycle, client.projection?.pendingPermission, commandPaletteOpen, credentialOpen, mobileRailOpen, modelPickerOpen, store]);

  useEffect(() => {
    if (previousRouteRef.current === route) return;
    previousRouteRef.current = route;
    queueMicrotask(() => document.getElementById("main-content")?.focus({ preventScroll: true }));
  }, [route]);

  const projection = client.projection;
  const activeSession = projection?.activeSession;
  const activeSessionId = activeSession?.id;
  const pendingPermissionIdentity = projection?.pendingPermission
    ? JSON.stringify([
        projection.pendingPermission.sessionId,
        projection.pendingPermission.id,
      ])
    : undefined;
  const permissionAnswering = pendingPermissionIdentity !== undefined
    && answeringPermissionIdentity === pendingPermissionIdentity;
  const blockingDialogOpen = Boolean(projection?.pendingPermission) || modelPickerOpen || credentialOpen || commandPaletteOpen;
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
      setCommandPaletteOpen(false);
      setMobileRailOpen(false);
    }
    setAnsweringPermissionIdentity((current) => (
      current === pendingPermissionIdentity ? current : undefined
    ));
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

  useEffect(() => {
    if (compactInspectorViewport || (client.projection?.settings.zoomPercent.value ?? 100) >= 150) {
      setInspectorOpen(false);
    }
  }, [client.projection?.settings.zoomPercent.value, compactInspectorViewport]);

  if (client.lifecycle === "failed") {
    return (
      <main className="fatalSurface">
        <span className="fatalIcon"><Icon name="warning" size={25} /></span>
        <p className="eyebrow">Desktop client unavailable</p>
        <h1>AutoHarness could not open the local runtime</h1>
        <p>{client.commandError ?? "The renderer did not receive an authoritative startup snapshot."}</p>
        <Button icon="refresh" onClick={() => void store.requestResync()} variant="primary">Try again</Button>
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

  const settings = projection.settings;
  const themePreference = settings.themePreset.value;
  const resolvedTheme = themePreference === "system" ? (systemDarkTheme ? "system" : "light") : themePreference;
  const colorMode = settings.colorMode.value;
  const reduceMotion = settings.reducedMotion.value || systemReducedMotion;
  const zoomFactor = settings.zoomPercent.value / 100;
  const appStyle = {
    "--app-zoom": zoomFactor,
    "--app-zoom-inverse": `${100 / zoomFactor}%`,
  } as CSSProperties;

  const openSession = (sessionId: string) => {
    if (sessionId !== projection.activeSessionId) void store.dispatch({ type: "open_session", sessionId });
    setRoute("chat");
  };

  const commandItems: readonly CommandItem[] = [
    { id: "new-session", label: "New session", description: "Create a durable conversation", icon: "new", shortcut: "Ctrl N", keywords: "create chat" },
    { id: "chat", label: "Open chat", description: "Return to the active conversation", icon: "chat", shortcut: "Alt 1" },
    { id: "sessions", label: "Browse sessions", description: "Search durable conversation history", icon: "sessions", shortcut: "Alt 2" },
    { id: "providers", label: "Manage providers", description: "Configure profiles, credentials, and model defaults", icon: "providers", shortcut: "Alt 3" },
    { id: "memory", label: "Open memory", description: "Inspect the knowledge workspace preview", icon: "memory", shortcut: "Alt 4" },
    { id: "settings", label: "Open settings", description: "Inspect and change renderer preferences", icon: "settings", shortcut: "Alt 5" },
    { id: "choose-model", label: "Choose model", description: "Open the compatible model catalog", icon: "model" },
    { id: "find-transcript", label: "Find in transcript", description: "Search messages, tools, paths, and results", icon: "search", shortcut: "Ctrl F", keywords: "conversation search" },
    { id: "export-transcript", label: "Export active transcript", description: "Write replayable history to Markdown", icon: "download", keywords: "save markdown" },
    { id: "toggle-inspector", label: inspectorOpen ? "Close inspector" : "Open inspector", description: "Toggle context and runtime details", icon: "inspect" },
  ];

  const runCommand = (command: string) => {
    if (command === "new-session") {
      void store.dispatch({ type: "create_session" });
      setRoute("chat");
    } else if (command === "choose-model") {
      setModelPickerOpen(true);
    } else if (command === "toggle-inspector") {
      setRoute("chat");
      setInspectorOpen((open) => !open);
    } else if (command === "find-transcript") {
      setRoute("chat");
      setTranscriptSearchRequest((value) => value + 1);
    } else if (command === "export-transcript") {
      if (activeSession) void store.dispatchAndWait({ type: "export_transcript", sessionId: activeSession.id });
    } else if (command === "chat" || command === "sessions" || command === "providers" || command === "memory" || command === "settings") {
      setRoute(command);
    }
  };

  const routeWorkspace =
    route === "chat" ? (
      <Conversation
        catalog={projection.catalog}
        connection={projection.connection}
        draft={activeDraft}
        interactionBlocked={blockingDialogOpen}
        model={activeModel}
        optimisticPrompts={client.optimisticPrompts}
        searchRequest={transcriptSearchRequest}
        submissionBehavior={settings.composerSubmitBehavior.value}
        timestampStyle={settings.timestampStyle.value}
        onCancel={(attemptId) => {
          if (activeSession) void store.dispatch({ type: "cancel_attempt", sessionId: activeSession.id, attemptId });
        }}
        onDraftChange={setActiveDraft}
        onOpenCredential={() => setCredentialOpen(true)}
        onOpenInspector={() => setInspectorOpen(true)}
        onOpenModelPicker={() => setModelPickerOpen(true)}
        onOpenNavigation={() => setMobileRailOpen(true)}
        onRefresh={() => void store.dispatch({ type: "refresh_catalog" })}
        onExport={() => activeSession ? store.dispatchAndWait({ type: "export_transcript", sessionId: activeSession.id }) : Promise.resolve("rejected")}
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
      <SessionsWorkspace
        onCommand={(command) => store.dispatchAndWait(command)}
        onOpen={openSession}
        onOpenNavigation={() => setMobileRailOpen(true)}
        snapshot={projection}
        timestampStyle={settings.timestampStyle.value}
      />
    ) : route === "providers" ? (
      <ProvidersWorkspace
        interactionBlocked={Boolean(projection.pendingPermission)}
        notice={client.notice}
        onCommand={(command) => store.dispatchAndWait(command)}
        onCredential={async (submission) => Boolean(await store.submitCredential(submission))}
        onOpenNavigation={() => setMobileRailOpen(true)}
        onStartAuthentication={async () => (await store.dispatch({ type: "start_codex_authentication" }))?.requestId}
        snapshot={projection}
      />
    ) : route === "memory" ? (
      <MemoryWorkspace onOpenNavigation={() => setMobileRailOpen(true)} />
    ) : (
      <SettingsWorkspace
        onCommand={(command) => store.dispatchAndWait(command)}
        onOpenNavigation={() => setMobileRailOpen(true)}
        settings={settings}
      />
    );

  return (
    <div
      className="app"
      data-color-mode={colorMode}
      data-density={settings.density.value}
      data-font-size={settings.fontSize.value}
      data-high-contrast={colorMode === "high-contrast"}
      data-reduce-motion={reduceMotion}
      data-theme={resolvedTheme}
      data-theme-preference={themePreference}
      data-zoom={settings.zoomPercent.value}
      style={appStyle}
    >
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
        onWidthChange={setRailWidth}
        runtimeMode={projection.runtimeMode}
        sessions={projection.sessions}
        width={railWidth}
        />
        <div className="workspaceSurface" ref={workspaceRef}>
          {route === "chat" && inspectorOpen ? (
            <SplitPane
              label="Resize context inspector"
              onValueChange={setInspectorPercent}
              secondary={
                <ContextInspector
                  activity={projection.activity}
                  connection={projection.connection}
                  mobileOpen={inspectorOpen}
                  model={activeModel}
                  onClose={() => setInspectorOpen(false)}
                  runtimeMode={projection.runtimeMode}
                  session={activeSession}
                />
              }
              value={inspectorPercent}
            >
              {routeWorkspace}
            </SplitPane>
          ) : routeWorkspace}
        </div>
      </div>

      {commandPaletteOpen && !projection.pendingPermission ? (
        <CommandPalette items={commandItems} onClose={() => setCommandPaletteOpen(false)} onSelect={runCommand} />
      ) : null}

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
            const receipt = await store.submitCredential({
              connectionId,
              operation: "session_only",
              credential,
            });
            return Boolean(receipt);
          }}
          providerLabel={projection.connection.kind === "offline" ? "provider" : projection.connection.providerLabel}
        />
      ) : null}

      {projection.pendingPermission ? (
        <PermissionDialog
          busy={permissionAnswering}
          key={pendingPermissionIdentity}
          onAllow={() => {
            const identity = pendingPermissionIdentity;
            const permission = projection.pendingPermission;
            if (permissionAnswering || !identity || !permission) return;
            setAnsweringPermissionIdentity(identity);
            void store.dispatchAndWait({
              type: "answer_permission",
              sessionId: permission.sessionId,
              toolCallId: permission.id,
              decision: "allow_once",
            }).then((outcome) => {
              if (outcome !== "committed") {
                setAnsweringPermissionIdentity((current) => (
                  current === identity ? undefined : current
                ));
              }
            });
          }}
          onDeny={() => {
            const identity = pendingPermissionIdentity;
            const permission = projection.pendingPermission;
            if (permissionAnswering || !identity || !permission) return;
            setAnsweringPermissionIdentity(identity);
            void store.dispatchAndWait({
              type: "answer_permission",
              sessionId: permission.sessionId,
              toolCallId: permission.id,
              decision: "deny",
            }).then((outcome) => {
              if (outcome !== "committed") {
                setAnsweringPermissionIdentity((current) => (
                  current === identity ? undefined : current
                ));
              }
            });
          }}
          permission={projection.pendingPermission}
        />
      ) : null}

      <div aria-atomic="true" aria-live="polite" className="statusAnnouncer">
        {client.notice?.message}
      </div>
      <div aria-atomic="true" aria-live="polite" className="srOnly" role="status">
        {`${route === "chat" ? "Chat" : route === "sessions" ? "Sessions" : route === "providers" ? "Providers" : route === "memory" ? "Memory" : "Settings"} workspace opened.`}
      </div>
      {client.commandError || client.notice?.level === "error" ? (
        <div className="toast" data-intent="error" role="alert"><Icon name="warning" size={16} /><span>{client.commandError ?? client.notice?.message}</span></div>
      ) : null}
    </div>
  );
}

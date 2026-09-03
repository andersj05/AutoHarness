import { useEffect, useMemo, useRef, useState, type FormEvent } from "react";
import type {
  ClientCommand,
  ClientSnapshot,
  CommandOutcome,
  CredentialSubmission,
  ProviderConfiguration,
  ProviderKind,
  ProviderProfile,
  ProviderStatus,
  ReasoningEffort,
} from "../protocol";
import type { ClientNotice } from "../store/clientStore";
import { Icon } from "./Icon";
import { Button, Callout, Chip, Field } from "./primitives";

type EditorMode = "create" | "edit" | "duplicate";

interface ProfileDraft {
  id: string;
  kind: Exclude<ProviderKind, "codex_subscription">;
  baseUrl: string;
  project: string;
  authHeader: string;
}

interface EditorState {
  mode: EditorMode;
  sourceId?: string;
  draft: ProfileDraft;
}

interface ProvidersWorkspaceProps {
  interactionBlocked: boolean;
  notice?: ClientNotice;
  snapshot: ClientSnapshot;
  onCommand: (command: ClientCommand) => Promise<CommandOutcome>;
  onCredential: (submission: CredentialSubmission) => Promise<boolean>;
  onOpenNavigation: () => void;
  onStartAuthentication: () => Promise<string | undefined>;
}

const REASONING_EFFORTS: readonly [ReasoningEffort | "", string][] = [
  ["", "Provider default"],
  ["none", "None"],
  ["minimal", "Minimal"],
  ["low", "Low"],
  ["medium", "Medium"],
  ["high", "High"],
  ["xhigh", "Extra high"],
  ["max", "Maximum"],
];

const STATUS_LABELS: Record<ProviderStatus, string> = {
  disconnected: "Disconnected",
  credential_required: "Credential needed",
  untested: "Untested",
  connecting: "Testing",
  ready: "Ready",
  offline: "Offline",
  failed: "Failed",
};

function profileKindLabel(kind: ProviderKind): string {
  if (kind === "gemini") return "Google AI Studio";
  if (kind === "router") return "OpenAI-compatible router";
  return "Codex subscription";
}

function statusIntent(status: ProviderStatus): "neutral" | "info" | "success" | "warning" | "danger" {
  if (status === "ready") return "success";
  if (status === "connecting") return "info";
  if (status === "credential_required" || status === "untested") return "warning";
  if (status === "failed") return "danger";
  return "neutral";
}

function credentialSourceLabel(profile: ProviderProfile): string {
  if (profile.credentialSource === "environment") return "Environment override";
  if (profile.credentialSource === "vault") return "Operating-system vault";
  if (profile.credentialSource === "session_only") return "Session memory only";
  return "No effective credential";
}

function emptyDraft(kind: ProfileDraft["kind"] = "gemini"): ProfileDraft {
  return { id: "", kind, baseUrl: "", project: "", authHeader: "" };
}

function profileDraft(profile: ProviderProfile): ProfileDraft {
  return {
    id: profile.id,
    kind: profile.configuration.kind === "router" ? "router" : "gemini",
    baseUrl: profile.configuration.baseUrl ?? "",
    project: profile.configuration.project ?? "",
    authHeader: profile.configuration.authHeader ?? "",
  };
}

function profileNameError(value: string, profiles: readonly ProviderProfile[], originalId?: string): string | undefined {
  if (!value) return "Enter a profile name.";
  if (value.length > 64) return "Keep the profile name within 64 visible ASCII characters.";
  if ([...value].some((character) => {
    const code = character.charCodeAt(0);
    return code < 33 || code > 126 || character === '"';
  })) {
    return "Use visible ASCII without spaces or quotation marks.";
  }
  if (value !== originalId && profiles.some((profile) => profile.id === value)) {
    return "A profile with this exact name already exists.";
  }
  return undefined;
}

function routerUrlError(value: string): string | undefined {
  if (!value) return "Enter the router base URL.";
  if (new TextEncoder().encode(value).length > 2_048) return "Keep the base URL within 2,048 UTF-8 bytes.";
  if (value.trim() !== value) return "Remove spaces before or after the base URL.";
  let parsed: URL;
  try {
    parsed = new URL(value);
  } catch {
    return "Enter a valid absolute URL.";
  }
  if (parsed.protocol !== "https:" && parsed.protocol !== "http:") return "Use an HTTPS URL, or HTTP for a loopback host.";
  if (parsed.username || parsed.password || parsed.search || parsed.hash) return "Do not include credentials, a query, or a fragment in this URL.";
  if (!parsed.pathname.endsWith("/")) return "End the base URL path with a slash.";
  const host = parsed.hostname.replace(/^\[|\]$/g, "").toLocaleLowerCase();
  const loopback = host === "localhost" || host === "::1" || /^127(?:\.|$)/.test(host);
  if (parsed.protocol === "http:" && !loopback) return "Plain HTTP is allowed only for localhost or another loopback address.";
  return undefined;
}

function optionalFieldError(label: string, value: string): string | undefined {
  if (new TextEncoder().encode(value).length > 256) return `Keep ${label} within 256 UTF-8 bytes.`;
  if ([...value].some((character) => /[\u0000-\u001f\u007f]/.test(character))) return `${label} cannot contain control characters.`;
  return undefined;
}

function authHeaderError(value: string): string | undefined {
  const bounded = optionalFieldError("the header name", value);
  if (bounded || !value) return bounded;
  if (!/^[!#$%&'*+\-.^_`|~0-9A-Za-z]+$/.test(value)) return "Enter one valid HTTP header name.";
  if (["accept", "connection", "content-length", "content-type", "host", "transfer-encoding"].includes(value.toLocaleLowerCase())) {
    return "Choose an authentication header rather than a transport-controlled header.";
  }
  return undefined;
}

function configurationFor(draft: ProfileDraft): ProviderConfiguration {
  if (draft.kind === "gemini") return { kind: "gemini" };
  const project = draft.project.trim();
  const authHeader = draft.authHeader.trim();
  return {
    kind: "router",
    baseUrl: draft.baseUrl.trim(),
    ...(project ? { project } : {}),
    ...(authHeader ? { authHeader } : {}),
  };
}

function providerEndpoint(profile: ProviderProfile): string {
  if (profile.configuration.kind === "router") return profile.configuration.baseUrl ?? "Router URL unavailable";
  if (profile.configuration.kind === "codex_subscription") return "Native Codex OAuth flow";
  return "Google AI Studio API";
}

export function ProvidersWorkspace({
  interactionBlocked,
  notice,
  snapshot,
  onCommand,
  onCredential,
  onOpenNavigation,
  onStartAuthentication,
}: ProvidersWorkspaceProps) {
  const [selectedId, setSelectedId] = useState(() => snapshot.providers.find((profile) => profile.active)?.id ?? snapshot.providers[0]?.id);
  const [editor, setEditor] = useState<EditorState>();
  const [deleteTargetId, setDeleteTargetId] = useState<string>();
  const [deleteConfirmation, setDeleteConfirmation] = useState("");
  const [credential, setCredential] = useState("");
  const [busyAction, setBusyAction] = useState<string>();
  const [actionMessage, setActionMessage] = useState<string>();
  const [authenticationRequestId, setAuthenticationRequestId] = useState<string>();
  const [defaultModelId, setDefaultModelId] = useState("");
  const [reasoningEffort, setReasoningEffort] = useState<ReasoningEffort | "">("");
  const credentialRef = useRef<HTMLInputElement>(null);

  const selected = snapshot.providers.find((profile) => profile.id === selectedId);
  const deleteTarget = snapshot.providers.find((profile) => profile.id === deleteTargetId);
  const responseActive = snapshot.activeSession?.attempt.kind === "streaming" || snapshot.activeSession?.attempt.kind === "cancelling";

  useEffect(() => {
    if (selectedId && snapshot.providers.some((profile) => profile.id === selectedId)) return;
    setSelectedId(snapshot.providers.find((profile) => profile.active)?.id ?? snapshot.providers[0]?.id);
  }, [selectedId, snapshot.providers]);

  useEffect(() => {
    setDefaultModelId(selected?.defaultModelId ?? "");
    setReasoningEffort(selected?.defaultReasoningEffort ?? "");
    setCredential("");
    if (credentialRef.current) credentialRef.current.value = "";
  }, [selected?.defaultModelId, selected?.defaultReasoningEffort, selected?.id]);

  useEffect(() => {
    if (!interactionBlocked) return;
    setCredential("");
    if (credentialRef.current) credentialRef.current.value = "";
  }, [interactionBlocked]);

  useEffect(() => {
    if (!authenticationRequestId || notice?.requestId !== authenticationRequestId) return;
    if (notice.code === "authentication_completed" || notice.level === "error") {
      setAuthenticationRequestId(undefined);
      setBusyAction(undefined);
    }
  }, [authenticationRequestId, notice]);

  const selectedModel = snapshot.catalog.models.find((model) => model.id === defaultModelId);
  const savedModelUnavailable = Boolean(defaultModelId) && !selectedModel;
  const defaultsDirty = selected ? (
    defaultModelId !== (selected.defaultModelId ?? "")
    || reasoningEffort !== (selected.defaultReasoningEffort ?? "")
  ) : false;

  const editorErrors = useMemo(() => {
    if (!editor) return {};
    const originalId = editor.mode === "edit" ? editor.sourceId : undefined;
    return {
      id: profileNameError(editor.draft.id, snapshot.providers, originalId),
      baseUrl: editor.draft.kind === "router" ? routerUrlError(editor.draft.baseUrl) : undefined,
      project: editor.draft.kind === "router" ? optionalFieldError("the project identity", editor.draft.project) : undefined,
      authHeader: editor.draft.kind === "router" ? authHeaderError(editor.draft.authHeader) : undefined,
    };
  }, [editor, snapshot.providers]);
  const editorInvalid = Object.values(editorErrors).some(Boolean);
  const credentialError = credential && (!credential.trim() || credential.length > 4_096 || [...credential].some((character) => {
    const code = character.charCodeAt(0);
    return code < 33 || code > 126;
  }))
    ? "Credentials must contain 1 to 4,096 visible ASCII characters without spaces."
    : undefined;

  const run = async (key: string, command: ClientCommand, success: string): Promise<CommandOutcome> => {
    if (busyAction || interactionBlocked) return "rejected";
    setBusyAction(key);
    setActionMessage(undefined);
    const outcome = await onCommand(command);
    setBusyAction(undefined);
    if (outcome === "committed") setActionMessage(success);
    return outcome;
  };

  const selectProfile = (id: string) => {
    setSelectedId(id);
    setEditor(undefined);
    setDeleteTargetId(undefined);
    setDeleteConfirmation("");
    setActionMessage(undefined);
  };

  const saveEditor = async (event: FormEvent) => {
    event.preventDefault();
    if (!editor || editorInvalid) return;
    const destinationId = editor.draft.id;
    const command: ClientCommand = editor.mode === "duplicate"
      ? { type: "duplicate_provider_profile", sourceId: editor.sourceId ?? "", destinationId }
      : {
          type: "upsert_provider_profile",
          profile: { id: destinationId, configuration: configurationFor(editor.draft) },
        };
    const verb = editor.mode === "edit" ? "Updated" : editor.mode === "duplicate" ? "Duplicated" : "Created";
    const outcome = await run("save-profile", command, `${verb} provider profile “${destinationId}”.`);
    if (outcome === "committed") {
      setSelectedId(destinationId);
      setEditor(undefined);
    }
  };

  const submitCredential = async (operation: CredentialSubmission["operation"]) => {
    if (!selected || !credential || credentialError || busyAction || interactionBlocked) return;
    const ownedCredential = credential;
    setCredential("");
    if (credentialRef.current) credentialRef.current.value = "";
    setBusyAction(`credential-${operation}`);
    setActionMessage(undefined);
    const accepted = await onCredential({ connectionId: selected.id, operation, credential: ownedCredential });
    setBusyAction(undefined);
    setActionMessage(accepted
      ? "Credential transferred to the native host and cleared from this page."
      : "The native host did not accept the credential transfer.");
  };

  const startAuthentication = async () => {
    if (authenticationRequestId || busyAction || interactionBlocked) return;
    setBusyAction("codex-auth");
    setActionMessage(undefined);
    const requestId = await onStartAuthentication();
    setBusyAction(undefined);
    if (requestId) {
      setAuthenticationRequestId(requestId);
      setActionMessage("Codex sign-in started in a native browser window.");
    }
  };

  const cancelAuthentication = async () => {
    if (!authenticationRequestId) return;
    const requestId = authenticationRequestId;
    const outcome = await run(
      "codex-cancel",
      { type: "cancel_codex_authentication", authenticationRequestId: requestId },
      "Codex sign-in cancelled.",
    );
    if (outcome === "committed") setAuthenticationRequestId(undefined);
  };

  const saveDefaults = async (event: FormEvent) => {
    event.preventDefault();
    if (!selected || !defaultModelId || !selected.active || selected.scope !== "named") return;
    const command: ClientCommand = reasoningEffort
      ? { type: "set_provider_defaults", connectionId: selected.id, modelId: defaultModelId, reasoningEffort }
      : { type: "set_provider_defaults", connectionId: selected.id, modelId: defaultModelId };
    await run("defaults", command, `Saved model defaults for “${selected.displayName}”.`);
  };

  const openCreate = () => {
    setEditor({ mode: "create", draft: emptyDraft() });
    setDeleteTargetId(undefined);
    setActionMessage(undefined);
  };

  return (
    <main className="routeWorkspace providerRouteWorkspace" id="main-content">
      <header className="routeWorkspaceHeader providerWorkspaceHeader">
        <button aria-label="Open navigation" className="iconButton mobileMenu" onClick={onOpenNavigation} type="button"><Icon name="menu" /></button>
        <div>
          <p className="eyebrow">Runtime connections</p>
          <h1>Providers</h1>
          <p>Manage named profiles, secret-safe credentials, model defaults, and content-free connection tests.</p>
        </div>
        <div className="providerHeaderActions">
          <Button icon="new" onClick={openCreate}>Add profile</Button>
          {authenticationRequestId ? (
            <Button loading={busyAction === "codex-cancel"} loadingLabel="Cancelling" onClick={() => void cancelAuthentication()} variant="secondary">Cancel Codex sign-in</Button>
          ) : (
            <Button icon="providers" loading={busyAction === "codex-auth"} loadingLabel="Starting sign-in" onClick={() => void startAuthentication()} variant="primary">Connect Codex</Button>
          )}
        </div>
      </header>

      {snapshot.providerRecoveryPending !== "0" ? (
        <Callout
          detail={`${snapshot.providerRecoveryPending} credential-vault operation${snapshot.providerRecoveryPending === "1" ? "" : "s"} require safe cleanup. Profile mutations remain blocked until recovery finishes.`}
          intent="warning"
          title="Credential recovery is pending"
        />
      ) : null}

      {authenticationRequestId ? (
        <Callout
          action={<Button loading={busyAction === "codex-cancel"} loadingLabel="Cancelling" onClick={() => void cancelAuthentication()} size="small">Cancel sign-in</Button>}
          detail={notice?.requestId === authenticationRequestId && notice.code === "authentication_browser_opened"
            ? "The native host opened your browser. Finish authentication there, then return to AutoHarness."
            : "Waiting for the native Codex authentication flow. No token passes through the webview."}
          icon="providers"
          intent="info"
          title="Codex sign-in in progress"
        />
      ) : null}

      <div className="providersWorkspaceGrid">
        <section aria-labelledby="provider-list-heading" className="providerListPane">
          <header><div><p className="eyebrow">Profiles</p><h2 id="provider-list-heading">Connections</h2></div><Chip intent="neutral">{snapshot.providers.length}</Chip></header>
          <div aria-label="Provider profiles" className="providerProfileList">
            {snapshot.providers.map((profile) => (
              <button
                aria-current={selectedId === profile.id ? "true" : undefined}
                className="providerProfileRow"
                data-active={profile.active}
                data-selected={selectedId === profile.id}
                key={profile.id}
                onClick={() => selectProfile(profile.id)}
                type="button"
              >
                <span className="providerRowIcon"><Icon name={profile.configuration.kind === "router" ? "branch" : profile.configuration.kind === "codex_subscription" ? "terminal" : "spark"} /></span>
                <span className="providerRowCopy"><strong>{profile.displayName}</strong><small>{profile.scope === "session_default" ? "Session default" : profileKindLabel(profile.configuration.kind)}</small></span>
                <Chip intent={statusIntent(profile.status)}>{profile.active ? "Active" : STATUS_LABELS[profile.status]}</Chip>
              </button>
            ))}
            {snapshot.providers.length === 0 ? <div className="providerListEmpty"><Icon name="providers" /><strong>No provider profiles</strong><p>Add Gemini or a router profile, or connect a Codex subscription.</p></div> : null}
          </div>
          <Button className="providerListAdd" icon="new" onClick={openCreate} variant="quiet">New named profile</Button>
        </section>

        <section aria-label="Provider profile details" className="providerDetailPane">
          {editor ? (
            <form className="providerEditor" onSubmit={(event) => void saveEditor(event)}>
              <header>
                <span className="providerDetailIcon"><Icon name={editor.mode === "duplicate" ? "copy" : "providers"} /></span>
                <div><p className="eyebrow">{editor.mode === "edit" ? "Non-secret settings" : editor.mode === "duplicate" ? "Independent copy" : "New connection"}</p><h2>{editor.mode === "edit" ? `Edit “${editor.draft.id}”` : editor.mode === "duplicate" ? "Duplicate profile" : "Add provider profile"}</h2></div>
              </header>
              <div className="providerEditorFields">
                <Field
                  autoCapitalize="none"
                  autoComplete="off"
                  autoCorrect="off"
                  data-initial-focus
                  disabled={editor.mode === "edit"}
                  error={editor.draft.id ? editorErrors.id : undefined}
                  hint={editor.mode === "edit" ? "Stable profile identity cannot be renamed." : "Up to 64 visible ASCII characters without spaces or quotation marks."}
                  label="Profile name"
                  onChange={(event) => setEditor({ ...editor, draft: { ...editor.draft, id: event.target.value } })}
                  spellCheck={false}
                  value={editor.draft.id}
                />
                {editor.mode !== "duplicate" ? (
                  <label className="providerSelectField">
                    <span>Provider type</span>
                    <select
                      aria-label="Provider type"
                      onChange={(event) => setEditor({ ...editor, draft: { ...editor.draft, kind: event.target.value as ProfileDraft["kind"] } })}
                      value={editor.draft.kind}
                    >
                      <option value="gemini">Google AI Studio</option>
                      <option value="router">OpenAI-compatible router</option>
                    </select>
                    <small>Codex subscriptions use the native browser sign-in flow.</small>
                  </label>
                ) : null}
                {editor.mode !== "duplicate" && editor.draft.kind === "router" ? (
                  <>
                    <Field error={editor.draft.baseUrl ? editorErrors.baseUrl : undefined} hint="HTTPS is required except for loopback development endpoints. Include a trailing slash." label="Base URL" onChange={(event) => setEditor({ ...editor, draft: { ...editor.draft, baseUrl: event.target.value } })} placeholder="https://router.example/v1/" spellCheck={false} type="url" value={editor.draft.baseUrl} />
                    <div className="providerFieldPair">
                      <Field error={editorErrors.project} hint="Optional stable cache and policy identity." label="Project identity" onChange={(event) => setEditor({ ...editor, draft: { ...editor.draft, project: event.target.value } })} placeholder="team-a" spellCheck={false} value={editor.draft.project} />
                      <Field error={editorErrors.authHeader} hint="Optional. Defaults to Authorization." label="Authentication header" onChange={(event) => setEditor({ ...editor, draft: { ...editor.draft, authHeader: event.target.value } })} placeholder="authorization" spellCheck={false} value={editor.draft.authHeader} />
                    </div>
                  </>
                ) : null}
                {editor.mode === "duplicate" ? <Callout detail="Only non-secret configuration and model defaults are copied. The new profile starts without credential linkage." icon="copy" title="Credentials stay separate" /> : null}
              </div>
              <footer className="providerEditorActions">
                <Button onClick={() => setEditor(undefined)} variant="quiet">Cancel</Button>
                <Button disabled={editorInvalid || interactionBlocked} loading={busyAction === "save-profile"} loadingLabel="Saving" type="submit" variant="primary">{editor.mode === "edit" ? "Save changes" : editor.mode === "duplicate" ? "Duplicate profile" : "Create profile"}</Button>
              </footer>
            </form>
          ) : selected ? (
            <>
              <header className="providerDetailHeader">
                <span className="providerDetailIcon"><Icon name={selected.configuration.kind === "router" ? "branch" : selected.configuration.kind === "codex_subscription" ? "terminal" : "spark"} /></span>
                <div><p className="eyebrow">{selected.scope === "session_default" ? "Temporary connection" : profileKindLabel(selected.configuration.kind)}</p><h2>{selected.displayName}</h2><code>{selected.id}</code></div>
                <Chip intent={statusIntent(selected.status)}>{STATUS_LABELS[selected.status]}</Chip>
              </header>

              {selected.safeError ? <Callout detail={selected.safeError} intent="danger" title="Connection test failed" /> : null}
              {selected.scope === "session_default" ? <Callout detail="This temporary row reflects process-level defaults. Create a named profile to save configuration, credentials, and model defaults." intent="info" title="Session default" /> : null}

              <dl className="providerFacts">
                <div><dt>Provider</dt><dd>{profileKindLabel(selected.configuration.kind)}</dd></div>
                <div><dt>Endpoint</dt><dd><code>{providerEndpoint(selected)}</code></dd></div>
                {selected.configuration.project ? <div><dt>Project</dt><dd><code>{selected.configuration.project}</code></dd></div> : null}
                {selected.configuration.authHeader ? <div><dt>Auth header</dt><dd><code>{selected.configuration.authHeader}</code></dd></div> : null}
                <div><dt>Credential source</dt><dd>{credentialSourceLabel(selected)}</dd></div>
                <div><dt>Saved credential</dt><dd>{selected.credentialState === "stored" ? "Linked in the operating-system vault" : selected.credentialState === "recovery_pending" ? "Recovery pending" : "No vault linkage"}</dd></div>
                <div><dt>Default model</dt><dd>{selected.defaultModelId ? <code>{snapshot.catalog.models.find((model) => model.id === selected.defaultModelId)?.displayName ?? selected.defaultModelId}</code> : "Provider default"}</dd></div>
              </dl>

              {selected.scope === "named" ? (
                <div className="providerPrimaryActions">
                  <div className="providerConnectionActions" data-single={selected.active}>
                    {!selected.active ? <Button disabled={responseActive || interactionBlocked} icon="bolt" loading={busyAction === "activate"} loadingLabel="Activating" onClick={() => void run("activate", { type: "activate_provider_profile", connectionId: selected.id }, `Activated “${selected.displayName}”.`)} size="small" variant="primary">Make active</Button> : null}
                    <Button disabled={selected.status === "connecting" || interactionBlocked} icon="refresh" loading={busyAction === "test"} loadingLabel="Testing" onClick={() => void run("test", { type: "test_provider_profile", connectionId: selected.id }, `Content-free connection test passed for “${selected.displayName}”.`)} size="small">Test connection</Button>
                  </div>
                  {selected.configuration.kind !== "codex_subscription" ? (
                    <div className="providerConfigurationActions">
                      <Button onClick={() => setEditor({ mode: "edit", sourceId: selected.id, draft: profileDraft(selected) })} size="small" variant="quiet">Edit</Button>
                      <Button onClick={() => setEditor({ mode: "duplicate", sourceId: selected.id, draft: { ...profileDraft(selected), id: `${selected.id}-copy` } })} size="small" variant="quiet">Duplicate</Button>
                    </div>
                  ) : null}
                </div>
              ) : null}
              {responseActive && !selected.active ? <p className="providerActionHint">Finish or cancel the active response before switching profiles.</p> : null}

              <section aria-labelledby="credential-heading" className="providerSection">
                <div className="providerSectionHeading"><div><p className="eyebrow">Secret boundary</p><h3 id="credential-heading">Credential</h3></div><Chip icon="shield" intent={selected.credentialSource === "none" ? "warning" : "success"}>{credentialSourceLabel(selected)}</Chip></div>
                {selected.credentialSource === "environment" ? <Callout detail={selected.credentialState === "stored" ? "The environment credential currently wins. The saved vault credential remains an encrypted fallback." : "The environment credential currently wins. You can save an encrypted fallback without exposing either value to the renderer."} icon="terminal" title="Environment override active" /> : null}
                {selected.configuration.kind === "codex_subscription" ? (
                  <div className="codexCredentialPanel">
                    <div><strong>Native browser authentication</strong><p>Rust opens and owns the sign-in flow. Tokens never pass through browser storage, frontend state, or diagnostics.</p></div>
                    {authenticationRequestId ? <Button onClick={() => void cancelAuthentication()}>Cancel sign-in</Button> : <Button icon="providers" onClick={() => void startAuthentication()} variant="primary">{selected.credentialSource === "none" ? "Connect subscription" : "Reconnect subscription"}</Button>}
                  </div>
                ) : (
                  <form className="providerCredentialForm" onSubmit={(event) => { event.preventDefault(); void submitCredential(selected.credentialState === "stored" ? "replace" : "save"); }}>
                    <Field
                      autoCapitalize="none"
                      autoComplete="off"
                      autoCorrect="off"
                      error={credentialError}
                      label="New provider credential"
                      leading={<Icon name="shield" size={16} />}
                      maxLength={4_096}
                      onChange={(event) => setCredential(event.target.value)}
                      placeholder="Paste credential"
                      ref={credentialRef}
                      spellCheck={false}
                      type="password"
                      value={credential}
                    />
                    <div className="providerCredentialBoundary"><Icon name="shield" size={16} /><p><strong>Immediate transfer and clear</strong><span>No snapshot, browser storage, transcript, diagnostic, or log receives this value.</span></p></div>
                    <div className="providerCredentialActions">
                      {selected.active ? <Button disabled={!credential || Boolean(credentialError) || interactionBlocked} loading={busyAction === "credential-session_only"} loadingLabel="Transferring" onClick={() => void submitCredential("session_only")} variant="quiet">Use this session</Button> : null}
                      {selected.scope === "named" ? <Button disabled={!credential || Boolean(credentialError) || interactionBlocked} loading={busyAction === `credential-${selected.credentialState === "stored" ? "replace" : "save"}`} loadingLabel="Transferring" type="submit" variant="primary">{selected.credentialState === "stored" ? selected.credentialSource === "environment" ? "Replace saved fallback" : "Replace saved credential" : selected.credentialSource === "environment" ? "Save fallback" : "Save credential"}</Button> : null}
                    </div>
                  </form>
                )}
                {(selected.credentialState === "stored" || selected.credentialSource === "session_only") && selected.scope === "named" ? (
                  <div className="providerDisconnectRow"><span>{selected.credentialSource === "environment" ? "Removing the saved fallback does not change the environment override." : "Disconnecting clears this profile's vault linkage and any session credential."}</span><Button loading={busyAction === "disconnect"} loadingLabel="Disconnecting" onClick={() => void run("disconnect", { type: "disconnect_provider_profile", connectionId: selected.id }, `Disconnected the saved credential for “${selected.displayName}”.`)} size="small" variant="quiet">{selected.credentialSource === "environment" ? "Remove saved fallback" : "Disconnect credential"}</Button></div>
                ) : null}
              </section>

              {selected.scope === "named" ? (
                <section aria-labelledby="defaults-heading" className="providerSection">
                  <div className="providerSectionHeading"><div><p className="eyebrow">Agent defaults</p><h3 id="defaults-heading">Model and reasoning</h3></div>{selected.active ? <Chip icon="bolt" intent="info">Active catalog</Chip> : <Chip intent="neutral">Activate to edit</Chip>}</div>
                  {selected.active ? (
                    <form className="providerDefaultsForm" onSubmit={(event) => void saveDefaults(event)}>
                      <label className="providerSelectField"><span>Default model</span><select aria-label="Default model" disabled={snapshot.catalog.status !== "ready" && snapshot.catalog.status !== "empty"} onChange={(event) => { setDefaultModelId(event.target.value); const model = snapshot.catalog.models.find((candidate) => candidate.id === event.target.value); if (model?.supportsReasoning === false) setReasoningEffort(""); }} value={defaultModelId}><option value="">Choose a model</option>{savedModelUnavailable ? <option value={defaultModelId}>Saved model unavailable in current catalog</option> : null}{snapshot.catalog.models.filter((model) => model.selectable).map((model) => <option key={model.id} value={model.id}>{model.displayName}</option>)}</select><small>{snapshot.catalog.status === "loading" ? "The host is refreshing the active provider catalog." : snapshot.catalog.status === "failed" ? snapshot.catalog.safeError ?? "The catalog could not load." : "Only models from the active profile's authoritative catalog are offered."}</small></label>
                      <label className="providerSelectField"><span>Reasoning effort</span><select aria-label="Reasoning effort" disabled={!defaultModelId || selectedModel?.supportsReasoning === false} onChange={(event) => setReasoningEffort(event.target.value as ReasoningEffort | "")} value={reasoningEffort}>{REASONING_EFFORTS.map(([value, label]) => <option key={value || "provider-default"} value={value}>{label}</option>)}</select><small>{selectedModel?.supportsReasoning === false ? "This model does not advertise reasoning control." : "Provider-native effort is saved atomically with the model."}</small></label>
                      <Button disabled={!defaultModelId || !defaultsDirty || interactionBlocked} loading={busyAction === "defaults"} loadingLabel="Saving defaults" type="submit" variant="primary">Save defaults</Button>
                    </form>
                  ) : <Callout detail="Activate this profile to load its authoritative model catalog before assigning defaults." icon="model" title="Defaults follow the active catalog" />}
                </section>
              ) : null}

              {selected.scope === "named" ? (
                <section className="providerDangerZone">
                  {deleteTarget?.id === selected.id ? (
                    <div><p><strong>Delete “{selected.displayName}” permanently?</strong><span>The profile is removed and its saved vault entry is scheduled for safe cleanup.</span></p><Field autoComplete="off" hint={`Type “${selected.id}” to confirm this exact profile.`} label="Confirm profile name" onChange={(event) => setDeleteConfirmation(event.target.value)} value={deleteConfirmation} /><div><Button onClick={() => { setDeleteTargetId(undefined); setDeleteConfirmation(""); }} variant="quiet">Cancel</Button><Button disabled={deleteConfirmation !== selected.id || interactionBlocked} loading={busyAction === "delete"} loadingLabel="Deleting" onClick={() => void run("delete", { type: "delete_provider_profile", connectionId: selected.id }, `Deleted provider profile “${selected.displayName}”.`).then((outcome) => { if (outcome === "committed") { setDeleteTargetId(undefined); setDeleteConfirmation(""); } })} variant="danger">Delete permanently</Button></div></div>
                  ) : <><span><strong>Delete profile</strong><small>Credential cleanup remains restart-safe if the vault is interrupted.</small></span><Button onClick={() => { setDeleteTargetId(selected.id); setDeleteConfirmation(""); }} size="small" variant="quiet">Delete</Button></>}
                </section>
              ) : null}

              <p aria-live="polite" className="providerActionMessage">{actionMessage}</p>
            </>
          ) : (
            <div className="providerDetailEmpty"><Icon name="providers" size={28} /><h2>Connect a provider</h2><p>Add a named Gemini or router profile, or use the native Codex subscription sign-in.</p><div><Button icon="new" onClick={openCreate}>Add profile</Button><Button icon="providers" onClick={() => void startAuthentication()} variant="primary">Connect Codex</Button></div></div>
          )}
        </section>
      </div>
    </main>
  );
}

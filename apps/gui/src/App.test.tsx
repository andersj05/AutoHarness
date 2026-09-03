import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";
import { App } from "./App";
import type { ClientCommand, ClientFrame, ClientTransport, CommandReceipt, CredentialSubmission } from "./protocol";
import { ClientStore } from "./store/clientStore";
import { createFixtureSnapshot, FixtureTransport, type FixtureScenario } from "./transport/fixtureTransport";

class RecordingFixtureTransport extends FixtureTransport {
  readonly commands: ClientCommand[] = [];
  readonly credentials: CredentialSubmission[] = [];

  override async command(command: ClientCommand): Promise<CommandReceipt> {
    this.commands.push(structuredClone(command));
    return super.command(command);
  }

  override async submitCredential(secret: CredentialSubmission): Promise<CommandReceipt> {
    this.credentials.push({ ...secret });
    return super.submitCredential(secret);
  }
}

class RejectingPromptTransport implements ClientTransport {
  listener?: (frame: ClientFrame) => void;
  readonly snapshotValue = createFixtureSnapshot("ready");

  async connect(listener: (frame: ClientFrame) => void) {
    this.listener = listener;
    return structuredClone(this.snapshotValue);
  }

  async command(command: ClientCommand): Promise<CommandReceipt> {
    const receipt = { requestId: "rejected-request" };
    if (command.type === "submit_prompt") {
      queueMicrotask(() => this.listener?.({
        kind: "notice",
        revision: "2",
        requestId: receipt.requestId,
        level: "error",
        code: "prompt_rejected",
        message: "The durable host rejected this prompt.",
      }));
    }
    return receipt;
  }

  async snapshot() { return structuredClone(this.snapshotValue); }
  async submitCredential() { return { requestId: "secret" }; }
  async close() {}
}

class ManualProjectionTransport implements ClientTransport {
  listener?: (frame: ClientFrame) => void;
  errorListener?: (error: unknown) => void;
  readonly commands: ClientCommand[] = [];
  private requestSequence = 0;
  private revision = 1n;

  constructor(readonly snapshotValue = createFixtureSnapshot("ready")) {}

  async connect(listener: (frame: ClientFrame) => void, onError: (error: unknown) => void) {
    this.listener = listener;
    this.errorListener = onError;
    return structuredClone(this.snapshotValue);
  }

  async command(command: ClientCommand): Promise<CommandReceipt> {
    this.commands.push(structuredClone(command));
    return { requestId: `manual-${++this.requestSequence}` };
  }

  settleLatest(committed: boolean) {
    this.revision += 1n;
    this.listener?.({
      kind: "notice",
      revision: this.revision.toString(),
      requestId: `manual-${this.requestSequence}`,
      level: committed ? "success" : "error",
      code: committed ? "command_committed" : "prompt_rejected",
      message: committed ? "The host committed the command." : "The host rejected the delayed prompt.",
    });
  }

  fail(error: unknown) {
    this.errorListener?.(error);
  }

  async snapshot() { return structuredClone(this.snapshotValue); }
  async submitCredential(_secret: CredentialSubmission) { return { requestId: `manual-secret-${++this.requestSequence}` }; }
  async close() {}
}

class FatalCommandTransport extends ManualProjectionTransport {
  override async command(command: ClientCommand): Promise<CommandReceipt> {
    this.commands.push(structuredClone(command));
    const failure = new Error("The native host disconnected during command dispatch.");
    this.fail(failure);
    throw failure;
  }
}

class AdmittedFailedPromptTransport extends ManualProjectionTransport {
  override async command(command: ClientCommand): Promise<CommandReceipt> {
    this.commands.push(structuredClone(command));
    const receipt = { requestId: "admitted-failed-prompt" };
    if (command.type === "submit_prompt") {
      const snapshot = structuredClone(this.snapshotValue);
      snapshot.transportRevision = "2";
      snapshot.activeSession = snapshot.activeSession
        ? {
            ...snapshot.activeSession,
            revision: "2",
            transcript: [
              ...snapshot.activeSession.transcript,
              {
                kind: "message",
                id: "input-admitted-failed",
                role: "user",
                content: command.prompt,
              },
            ],
            attempt: {
              kind: "failed",
              id: "attempt-admitted-failed",
              code: "context_not_committed",
              message: "The prompt is durable, but its provider attempt could not start.",
              retryable: true,
            },
          }
        : undefined;
      queueMicrotask(() => {
        this.listener?.({ kind: "snapshot", reason: "projection", revision: "2", snapshot });
        this.listener?.({
          kind: "notice",
          revision: "3",
          requestId: receipt.requestId,
          level: "success",
          code: "command_committed",
          message: "The durable host committed the prompt.",
        });
      });
    }
    return receipt;
  }
}

class CredentialRetryTransport extends ManualProjectionTransport {
  readonly credentials: CredentialSubmission[] = [];

  constructor() {
    super(createFixtureSnapshot("credential"));
  }

  override async submitCredential(secret: CredentialSubmission): Promise<CommandReceipt> {
    this.credentials.push({ ...secret });
    const requestId = `credential-${String(this.credentials.length)}`;
    if (this.credentials.length === 1) {
      const failed = structuredClone(this.snapshotValue);
      failed.transportRevision = "2";
      failed.catalog = {
        status: "failed",
        source: "none",
        models: [],
        safeError: "The entered credential was rejected.",
      };
      failed.connection = {
        kind: "credential_required",
        providerLabel: "Google AI Studio",
        reason: "The entered credential was rejected. Enter a replacement to continue.",
      };
      queueMicrotask(() => this.listener?.({
        kind: "snapshot",
        reason: "projection",
        revision: "2",
        snapshot: failed,
      }));
    } else {
      const ready = createFixtureSnapshot("ready");
      ready.transportRevision = "3";
      queueMicrotask(() => this.listener?.({
        kind: "snapshot",
        reason: "projection",
        revision: "3",
        snapshot: ready,
      }));
    }
    return { requestId };
  }
}

const stores: ClientStore[] = [];

function renderScenario(scenario: FixtureScenario) {
  const transport = new RecordingFixtureTransport(scenario);
  const store = new ClientStore(transport);
  stores.push(store);
  const user = userEvent.setup();
  render(<App store={store} />);
  return { store, transport, user };
}

function renderTransport(transport: ClientTransport, commandSettlementTimeoutMs?: number) {
  const store = new ClientStore(transport, commandSettlementTimeoutMs);
  stores.push(store);
  const user = userEvent.setup();
  render(<App store={store} />);
  return { store, user };
}

afterEach(async () => {
  cleanup();
  await Promise.all(stores.splice(0).map((store) => store.close()));
  vi.unstubAllGlobals();
});

describe("AutoHarness GUI", () => {
  it("renders the fixture shell with semantic navigation and an open conversation flow", async () => {
    renderScenario("ready");
    expect(await screen.findByRole("heading", { name: "Design the GUI migration" })).toBeInTheDocument();
    expect(screen.getByRole("navigation", { name: "Primary" })).toBeInTheDocument();
    expect(screen.getByRole("main")).toHaveAttribute("id", "main-content");
    expect(screen.getByText("React-free client store that repairs revision gaps.", { exact: false })).toBeInTheDocument();
    expect(screen.getByText("Browser fixture - simulated state only")).toBeInTheDocument();
    expect(screen.getAllByText("Browser fixture").length).toBeGreaterThan(0);
    expect(screen.queryByText("Rust-owned authority")).not.toBeInTheDocument();
  });

  it("opens the keyboard command palette and persists authoritative appearance settings", async () => {
    const { transport, user } = renderScenario("ready");
    await screen.findByRole("heading", { name: "Design the GUI migration" });
    await user.keyboard("{Control>}k{/Control}");
    expect(screen.getByRole("dialog", { name: "Go anywhere" })).toBeInTheDocument();
    expect(document.querySelector(".appShell")).toHaveAttribute("inert");
    await user.click(screen.getByRole("menuitem", { name: /Open settings/ }));

    await user.selectOptions(screen.getByRole("combobox", { name: "Theme identity" }), "rose");
    await user.selectOptions(screen.getByRole("combobox", { name: "Color and contrast" }), "no-color");
    await user.click(screen.getByRole("checkbox", { name: /Reduce motion/ }));

    const app = document.querySelector(".app");
    await waitFor(() => expect(app).toHaveAttribute("data-theme", "rose"));
    expect(app).toHaveAttribute("data-color-mode", "no-color");
    expect(app).toHaveAttribute("data-reduce-motion", "true");
    expect(transport.commands.filter((command) => command.type === "update_client_preference")).toHaveLength(3);
    expect(screen.getAllByText("your settings").length).toBeGreaterThanOrEqual(3);
  });

  it("exposes a keyboard-resizable context split pane on wide workspaces", async () => {
    renderScenario("ready");
    await screen.findByRole("heading", { name: "Design the GUI migration" });
    const separator = screen.getByRole("separator", { name: "Resize context inspector" });
    expect(separator).toHaveAttribute("aria-valuenow", "72");
    fireEvent.keyDown(separator, { key: "ArrowLeft" });
    expect(separator).toHaveAttribute("aria-valuenow", "70");
  });

  it("retains independently resizable navigation and inspector panes", async () => {
    const { user } = renderScenario("ready");
    await screen.findByRole("heading", { name: "Design the GUI migration" });
    const navigation = screen.getByRole("separator", { name: "Resize navigation" });
    expect(navigation).toHaveAttribute("aria-valuenow", "248");
    fireEvent.keyDown(navigation, { key: "ArrowRight", shiftKey: true });
    expect(navigation).toHaveAttribute("aria-valuenow", "272");

    await user.click(screen.getByRole("button", { name: "Sessions" }));
    expect(await screen.findByRole("heading", { name: "Sessions" })).toBeInTheDocument();
    expect(screen.getByRole("separator", { name: "Resize navigation" })).toHaveAttribute("aria-valuenow", "272");
    await user.click(screen.getByRole("button", { name: "Chat" }));
    expect(screen.getByRole("separator", { name: "Resize context inspector" })).toHaveAttribute("aria-valuenow", "72");
  });

  it("opens provider management from the primary application rail", async () => {
    const { user } = renderScenario("ready");
    await screen.findByRole("heading", { name: "Design the GUI migration" });
    await user.click(screen.getByRole("button", { name: "Providers" }));
    expect(await screen.findByRole("heading", { name: "Providers" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /Personal Gemini/ })).toHaveAttribute("aria-current", "true");
    expect(screen.getByRole("heading", { name: "Credential" })).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "Model and reasoning" })).toBeInTheDocument();
  });

  it("navigates primary routes by keyboard and restores focus to each main landmark", async () => {
    renderScenario("ready");
    await screen.findByRole("heading", { name: "Design the GUI migration" });

    fireEvent.keyDown(window, { key: "2", altKey: true });
    expect(await screen.findByRole("heading", { name: "Sessions" })).toBeInTheDocument();
    await waitFor(() => expect(screen.getByRole("main")).toHaveFocus());

    fireEvent.keyDown(window, { key: "5", altKey: true });
    expect(await screen.findByRole("heading", { name: "Settings" })).toBeInTheDocument();
    await waitFor(() => expect(screen.getByRole("main")).toHaveFocus());
    expect(screen.getByRole("complementary", { name: "Application navigation" })).toBeInTheDocument();
    expect(screen.getByRole("navigation", { name: "Settings sections" })).toBeInTheDocument();
  });

  it("follows operating-system theme and reduced-motion preferences", async () => {
    vi.stubGlobal("matchMedia", vi.fn((query: string) => ({
      matches: query.includes("prefers-color-scheme") || query.includes("prefers-reduced-motion"),
      media: query,
      onchange: null,
      addListener: vi.fn(),
      removeListener: vi.fn(),
      addEventListener: vi.fn(),
      removeEventListener: vi.fn(),
      dispatchEvent: vi.fn(),
    })));
    renderScenario("ready");
    await screen.findByRole("heading", { name: "Design the GUI migration" });
    expect(document.querySelector(".app")).toHaveAttribute("data-theme", "system");
    expect(document.querySelector(".app")).toHaveAttribute("data-theme-preference", "system");
    expect(document.querySelector(".app")).toHaveAttribute("data-reduce-motion", "true");
  });

  it("applies 200 percent zoom while preserving the settings route and its actions", async () => {
    const { user } = renderScenario("ready");
    await user.click(await screen.findByRole("button", { name: "Settings" }));
    await user.selectOptions(screen.getByRole("combobox", { name: "Interface zoom" }), "200");

    const app = document.querySelector<HTMLElement>(".app");
    await waitFor(() => expect(app).toHaveAttribute("data-zoom", "200"));
    expect(app?.style.getPropertyValue("--app-zoom")).toBe("2");
    expect(app?.style.getPropertyValue("--app-zoom-inverse")).toBe("50%");
    expect(screen.getByRole("button", { name: "Reset Interface zoom to its inherited value" })).toBeEnabled();
    expect(screen.getByRole("combobox", { name: "Submit prompts with" })).toBeVisible();
  });

  it("uses Ctrl or Cmd plus S for multiline submission after resetting the fixture override", async () => {
    const { transport, user } = renderScenario("ready");
    await user.click(await screen.findByRole("button", { name: "Settings" }));
    await user.click(screen.getByRole("button", { name: "Reset Submit prompts with to its inherited value" }));
    await waitFor(() => expect(screen.getByRole("combobox", { name: "Submit prompts with" })).toHaveValue("control_s"));
    await user.click(screen.getByRole("button", { name: "Chat" }));

    const composer = await screen.findByRole("textbox", { name: "Message AutoHarness" });
    await user.type(composer, "first line");
    await user.keyboard("{Enter}");
    await user.type(composer, "second line");
    expect(transport.commands.some((command) => command.type === "submit_prompt")).toBe(false);
    await user.keyboard("{Control>}s{/Control}");

    await waitFor(() => expect(transport.commands.some((command) => command.type === "submit_prompt")).toBe(true));
    expect(transport.commands.find((command) => command.type === "submit_prompt")).toMatchObject({
      prompt: "first line\nsecond line",
    });
  });

  it("applies density, conversation font, and timestamp preferences to primary surfaces", async () => {
    const { user } = renderScenario("ready");
    await user.click(await screen.findByRole("button", { name: "Settings" }));
    await user.selectOptions(screen.getByRole("combobox", { name: "Interface density" }), "compact");
    await user.selectOptions(screen.getByRole("combobox", { name: "Conversation font size" }), "extra_large");
    await user.selectOptions(screen.getByRole("combobox", { name: "Timestamps" }), "hidden");
    await user.click(screen.getByRole("button", { name: "Chat" }));

    const app = document.querySelector(".app");
    await waitFor(() => expect(app).toHaveAttribute("data-density", "compact"));
    expect(app).toHaveAttribute("data-font-size", "extra_large");
    expect(document.querySelector(".messageTurn time")).not.toBeInTheDocument();
  });

  it("preserves exact prompt whitespace and clears only after mailbox acceptance", async () => {
    const { transport, user } = renderScenario("ready");
    const composer = await screen.findByRole("textbox", { name: "Message AutoHarness" });
    await user.type(composer, "  preserve me  ");
    await user.keyboard("{Enter}");
    await waitFor(() => expect(transport.commands.some((command) => command.type === "submit_prompt")).toBe(true));
    const submit = transport.commands.find((command) => command.type === "submit_prompt");
    expect(submit).toMatchObject({ prompt: "  preserve me  " });
    expect(composer).toHaveValue("");
  });

  it("preempts lower-authority UI and suppresses ordinary shortcuts for permission", async () => {
    const { transport, user } = renderScenario("permission");
    const dialog = await screen.findByRole("dialog", { name: "Write one workspace file" });
    expect(dialog).toBeInTheDocument();
    const shell = document.querySelector<HTMLElement>(".appShell");
    expect(shell).toHaveAttribute("inert");
    expect(shell).not.toContainElement(dialog);
    expect(screen.queryByRole("dialog", { name: "Choose a model" })).not.toBeInTheDocument();
    await user.keyboard("{Control>}n{/Control}");
    expect(transport.commands.some((command) => command.type === "create_session")).toBe(false);
    await user.keyboard("{Control>}k{/Control}");
    expect(screen.queryByRole("dialog", { name: "Go anywhere" })).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Deny operation" })).toHaveFocus();
    await user.dblClick(screen.getByRole("button", { name: "Allow once" }));
    expect(transport.commands.filter((command) => command.type === "answer_permission")).toHaveLength(1);
  });

  it("resets consecutive permission focus and activation to deny", async () => {
    const initial = createFixtureSnapshot("permission");
    const transport = new ManualProjectionTransport(initial);
    const { store, user } = renderTransport(transport);
    const firstDeny = await screen.findByRole("button", { name: "Deny operation" });
    expect(firstDeny).toHaveFocus();
    await user.click(screen.getByRole("button", { name: "Allow once" }));
    expect(screen.getByRole("button", { name: "Recording answer" })).toBeDisabled();
    expect(transport.commands.filter((command) => command.type === "answer_permission")).toEqual([
      {
        type: "answer_permission",
        sessionId: initial.pendingPermission!.sessionId,
        toolCallId: initial.pendingPermission!.id,
        decision: "allow_once",
      },
    ]);

    store.applyFrame({
      kind: "snapshot",
      reason: "projection",
      revision: "2",
      snapshot: {
        ...initial,
        transportRevision: "2",
        pendingPermission: {
          ...initial.pendingPermission!,
          id: "tool-call-queued",
          capability: "Read one workspace file",
          resource: "docs/memory/project.md",
        },
      },
    });

    const nextDialog = await screen.findByRole("dialog", { name: "Read one workspace file" });
    expect(nextDialog).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Allow once" })).toBeEnabled();
    await waitFor(() => {
      expect(screen.getByRole("button", { name: "Deny operation" })).toHaveFocus();
    });
    expect(transport.commands.filter((command) => command.type === "answer_permission")).toHaveLength(1);

    await user.keyboard("{Enter}");

    expect(transport.commands.filter((command) => command.type === "answer_permission")).toEqual([
      {
        type: "answer_permission",
        sessionId: initial.pendingPermission!.sessionId,
        toolCallId: initial.pendingPermission!.id,
        decision: "allow_once",
      },
      {
        type: "answer_permission",
        sessionId: initial.pendingPermission!.sessionId,
        toolCallId: "tool-call-queued",
        decision: "deny",
      },
    ]);
  });

  it("keeps dialog input focus stable and suppresses repeated or modal-owned shortcuts", async () => {
    const { store, transport, user } = renderScenario("ready");
    await user.click((await screen.findAllByRole("button", { name: "Change model, current Gemini 2.5 Pro" }))[0]!);
    const search = screen.getByRole("searchbox", { name: "Search models" });
    expect(search).toHaveFocus();

    fireEvent.keyDown(window, { key: "n", ctrlKey: true });
    expect(transport.commands.some((command) => command.type === "create_session")).toBe(false);

    await store.dispatch({ type: "refresh_catalog" });
    expect(search).toHaveFocus();
    await user.keyboard("{Escape}");
    fireEvent.keyDown(window, { key: "n", ctrlKey: true, repeat: true });
    expect(transport.commands.some((command) => command.type === "create_session")).toBe(false);
  });

  it("restores the exact draft and surfaces a durable command rejection", async () => {
    const transport = new RejectingPromptTransport();
    const store = new ClientStore(transport);
    stores.push(store);
    const user = userEvent.setup();
    render(<App store={store} />);
    const composer = await screen.findByRole("textbox", { name: "Message AutoHarness" });
    await user.type(composer, "  keep on rejection  ");
    await user.keyboard("{Enter}");
    expect(await screen.findByRole("alert")).toHaveTextContent("The durable host rejected this prompt.");
    expect(composer).toHaveValue("  keep on rejection  ");
  });

  it("keeps newer text when a delayed submission commits", async () => {
    const transport = new ManualProjectionTransport();
    const { user } = renderTransport(transport);
    const composer = await screen.findByRole("textbox", { name: "Message AutoHarness" });
    await user.type(composer, "first draft");
    await user.keyboard("{Enter}");
    expect(composer).toHaveValue("");
    await user.type(composer, "newer text");
    transport.settleLatest(true);
    await waitFor(() => expect(composer).toHaveValue("newer text"));
  });

  it("restores a rejected delayed submission without overwriting newer text", async () => {
    const transport = new ManualProjectionTransport();
    const { user } = renderTransport(transport);
    const composer = await screen.findByRole("textbox", { name: "Message AutoHarness" });
    await user.type(composer, "rejected draft");
    await user.keyboard("{Enter}");
    await user.type(composer, "newer text");
    transport.settleLatest(false);
    await waitFor(() => expect(composer).toHaveValue("rejected draft\n\nnewer text"));
  });

  it("does not restore an uncertain prompt as a safely retryable draft", async () => {
    const transport = new ManualProjectionTransport();
    const { user } = renderTransport(transport, 10);
    const composer = await screen.findByRole("textbox", { name: "Message AutoHarness" });
    await user.type(composer, "this prompt may already be durable");
    await user.keyboard("{Enter}");

    expect(await screen.findByRole("alert")).toHaveTextContent(
      "The host did not confirm whether the command committed.",
    );
    expect(await screen.findByRole("textbox", { name: "Message AutoHarness" })).toHaveValue("");
  });

  it("keeps a durably admitted prompt cleared when its provider attempt fails to start", async () => {
    const transport = new AdmittedFailedPromptTransport();
    const { user } = renderTransport(transport);
    const composer = await screen.findByRole("textbox", { name: "Message AutoHarness" });
    const prompt = "persist exactly once before startup failure";
    await user.type(composer, prompt);
    await user.keyboard("{Enter}");

    expect(await screen.findByText(prompt)).toBeInTheDocument();
    expect(screen.getAllByText(prompt)).toHaveLength(1);
    expect(screen.getByText("The prompt is durable, but its provider attempt could not start.")).toBeInTheDocument();
    expect(screen.getByRole("textbox", { name: "Message AutoHarness" })).toHaveValue("");
  });

  it("clears a credential sentinel from the DOM immediately after one-way submission", async () => {
    const { store, transport, user } = renderScenario("credential");
    await user.click(await screen.findByRole("button", { name: "Enter credential" }));
    const input = screen.getByLabelText("Provider credential");
    const credentialDialog = screen.getByRole("dialog", { name: "Connect Google AI Studio" });
    const shell = document.querySelector<HTMLElement>(".appShell");
    expect(shell).toHaveAttribute("inert");
    expect(shell).not.toContainElement(credentialDialog);
    expect(input).toHaveFocus();
    await store.dispatch({ type: "refresh_catalog" });
    expect(input).toHaveFocus();
    const sentinel = "GUI_SECRET_SENTINEL_9f2a";
    await user.type(input, sentinel);
    await user.click(screen.getByRole("button", { name: "Connect provider" }));
    expect(input).toHaveValue("");
    await waitFor(() => expect(transport.credentials).toHaveLength(1));
    expect(transport.credentials[0]).toEqual({ connectionId: "connection-gemini", operation: "session_only", credential: sentinel });
    expect(document.body.textContent).not.toContain(sentinel);
    expect(window.localStorage.length).toBe(0);
    expect(window.sessionStorage.length).toBe(0);
  });

  it("reopens credential ingress after rejection and accepts a replacement without restart", async () => {
    const transport = new CredentialRetryTransport();
    const { user } = renderTransport(transport);
    await user.click(await screen.findByRole("button", { name: "Enter credential" }));
    await user.type(screen.getByLabelText("Provider credential"), "rejected-key");
    await user.click(screen.getByRole("button", { name: "Connect provider" }));

    expect(await screen.findByText("The entered credential was rejected. Enter a replacement to continue.")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Enter credential" }));
    await user.type(screen.getByLabelText("Provider credential"), "replacement-key");
    await user.click(screen.getByRole("button", { name: "Connect provider" }));

    await waitFor(() => expect(screen.getByText("connected")).toBeInTheDocument());
    expect(transport.credentials).toEqual([
      { connectionId: "connection-gemini", operation: "session_only", credential: "rejected-key" },
      { connectionId: "connection-gemini", operation: "session_only", credential: "replacement-key" },
    ]);
    expect(document.body.textContent).not.toContain("replacement-key");
  });

  it("offers credential ingress when provider auth is required with a ready cached catalog", async () => {
    const snapshot = createFixtureSnapshot("ready");
    snapshot.connection = {
      kind: "credential_required",
      providerLabel: "Google AI Studio",
      reason: "This provider profile needs a fresh credential.",
    };
    renderTransport(new ManualProjectionTransport(snapshot));
    expect(await screen.findByRole("button", { name: "Enter credential" })).toBeInTheDocument();
    expect(screen.getByText("This provider profile needs a fresh credential.")).toBeInTheDocument();
  });

  it("blocks prompt submission when the selected catalog model is not selectable", async () => {
    const snapshot = createFixtureSnapshot("ready");
    snapshot.catalog = {
      ...snapshot.catalog,
      models: snapshot.catalog.models.map((model) => ({
        ...model,
        selectable: model.id === snapshot.activeSession?.selectedModelId ? false : model.selectable,
      })),
    };
    const transport = new ManualProjectionTransport(snapshot);
    const { user } = renderTransport(transport);
    const composer = await screen.findByRole("textbox", { name: "Message AutoHarness" });
    await user.type(composer, "do not dispatch");
    expect(screen.getByText("The selected model is currently unavailable. Choose another model.")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Send message" })).toBeDisabled();
    await user.keyboard("{Enter}");
    expect(transport.commands.some((command) => command.type === "submit_prompt")).toBe(false);
  });

  it("blocks the stale shell after transport failure until resynchronization succeeds", async () => {
    const transport = new ManualProjectionTransport();
    const { user } = renderTransport(transport);
    const composer = await screen.findByRole("textbox", { name: "Message AutoHarness" });
    await user.type(composer, "draft survives recovery");

    transport.fail(new Error("The renderer lost its authoritative channel."));

    expect(await screen.findByRole("heading", { name: "AutoHarness could not open the local runtime" })).toBeInTheDocument();
    expect(screen.queryByRole("textbox", { name: "Message AutoHarness" })).not.toBeInTheDocument();
    fireEvent.keyDown(window, { key: "n", ctrlKey: true });
    expect(transport.commands).toHaveLength(0);

    await user.click(screen.getByRole("button", { name: "Try again" }));
    expect(await screen.findByRole("textbox", { name: "Message AutoHarness" })).toHaveValue("draft survives recovery");
    fireEvent.keyDown(window, { key: "n", ctrlKey: true });
    await waitFor(() => expect(transport.commands.some((command) => command.type === "create_session")).toBe(true));
  });

  it("keeps exact drafts scoped to their durable sessions", async () => {
    const initial = createFixtureSnapshot("ready");
    const sessionA = initial.activeSession!;
    const sessionB = {
      ...structuredClone(sessionA),
      id: "session-context",
      revision: "4",
      title: "Audit context manifests",
      transcript: [],
    };
    const transport = new ManualProjectionTransport(initial);
    const { store, user } = renderTransport(transport);
    const composerA = await screen.findByRole("textbox", { name: "Message AutoHarness" });
    await user.type(composerA, "draft for session A");

    store.applyFrame({
      kind: "snapshot",
      reason: "projection",
      revision: "2",
      snapshot: {
        ...initial,
        transportRevision: "2",
        activeSessionId: sessionB.id,
        activeSession: sessionB,
      },
    });

    await waitFor(() => expect(screen.getByRole("textbox", { name: "Message AutoHarness" })).toHaveValue(""));
    const composerB = screen.getByRole("textbox", { name: "Message AutoHarness" });
    await user.keyboard("{Enter}");
    expect(transport.commands.some((command) => command.type === "submit_prompt")).toBe(false);
    await user.type(composerB, "draft for session B");

    store.applyFrame({
      kind: "snapshot",
      reason: "projection",
      revision: "3",
      snapshot: {
        ...initial,
        transportRevision: "3",
        activeSessionId: sessionA.id,
        activeSession: sessionA,
      },
    });
    await waitFor(() => expect(screen.getByRole("textbox", { name: "Message AutoHarness" })).toHaveValue("draft for session A"));

    store.applyFrame({
      kind: "snapshot",
      reason: "projection",
      revision: "4",
      snapshot: {
        ...initial,
        transportRevision: "4",
        activeSessionId: sessionB.id,
        activeSession: sessionB,
      },
    });
    await waitFor(() => expect(screen.getByRole("textbox", { name: "Message AutoHarness" })).toHaveValue("draft for session B"));
  });

  it("blocks the shell after a fatal command carrier failure", async () => {
    const transport = new FatalCommandTransport();
    renderTransport(transport);
    expect(await screen.findByRole("textbox", { name: "Message AutoHarness" })).toBeInTheDocument();

    fireEvent.keyDown(window, { key: "n", ctrlKey: true });

    expect(await screen.findByRole("heading", { name: "AutoHarness could not open the local runtime" })).toBeInTheDocument();
    expect(transport.commands).toHaveLength(1);
    fireEvent.keyDown(window, { key: "n", ctrlKey: true });
    expect(transport.commands).toHaveLength(1);
  });

  it("keeps every modal outside an inert application shell", async () => {
    const { user } = renderScenario("ready");
    await user.click((await screen.findAllByRole("button", { name: "Change model, current Gemini 2.5 Pro" }))[0]!);
    const dialog = screen.getByRole("dialog", { name: "Choose a model" });
    const shell = document.querySelector<HTMLElement>(".appShell");
    expect(shell).toHaveAttribute("inert");
    expect(shell).not.toContainElement(dialog);
    await user.keyboard("{Escape}");
    await waitFor(() => expect(shell).not.toHaveAttribute("inert"));
  });

  it("isolates mobile navigation, focuses its close action, and restores focus on Escape", async () => {
    vi.stubGlobal("matchMedia", vi.fn((query: string) => ({
      matches: query.includes("1180") || query.includes("680"),
      media: query,
      onchange: null,
      addListener: vi.fn(),
      removeListener: vi.fn(),
      addEventListener: vi.fn(),
      removeEventListener: vi.fn(),
      dispatchEvent: vi.fn(),
    })));
    const { user } = renderScenario("ready");
    expect(await screen.findByRole("heading", { name: "Design the GUI migration" })).toBeInTheDocument();
    const rail = document.querySelector<HTMLElement>(".appRail");
    expect(screen.queryByRole("complementary", { name: "Context inspector" })).not.toBeInTheDocument();
    expect(rail).toHaveAttribute("aria-hidden", "true");
    expect(rail).toHaveAttribute("inert");

    const openNavigation = screen.getByRole("button", { name: "Open navigation" });
    await user.click(openNavigation);
    const closeNavigation = screen.getByRole("button", { name: "Close navigation" });
    expect(closeNavigation).toHaveFocus();
    expect(screen.getByRole("button", { name: "Dismiss navigation drawer" })).toBeInTheDocument();
    expect(document.querySelector(".workspaceSurface")).toHaveAttribute("inert");
    await user.keyboard("{Escape}");
    await waitFor(() => expect(rail).toHaveAttribute("aria-hidden", "true"));
    expect(openNavigation).toHaveFocus();
  });

  it.each([
    ["offline", "Fixture provider offline"],
    ["empty", "No compatible models"],
    ["failed", "Response interrupted"],
  ] as const)("exposes a concrete recovery action for the %s fixture", async (scenario, label) => {
    renderScenario(scenario);
    expect(await screen.findByText(label)).toBeInTheDocument();
  });
});

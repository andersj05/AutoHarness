import { cleanup, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { ClientCommand, ClientSnapshot, CredentialSubmission, ProviderProfile } from "../protocol";
import type { ClientNotice } from "../store/clientStore";
import { createFixtureSnapshot } from "../transport/fixtureTransport";
import { ProvidersWorkspace } from "./ProvidersWorkspace";

afterEach(cleanup);

function renderWorkspace(options: {
  snapshot?: ClientSnapshot;
  interactionBlocked?: boolean;
  notice?: ClientNotice;
  startRequestId?: string;
} = {}) {
  const commands: ClientCommand[] = [];
  const credentials: CredentialSubmission[] = [];
  const onCommand = vi.fn(async (command: ClientCommand) => {
    commands.push(structuredClone(command));
    return "committed" as const;
  });
  const onCredential = vi.fn(async (submission: CredentialSubmission) => {
    credentials.push(structuredClone(submission));
    return true;
  });
  const onStartAuthentication = vi.fn(async () => options.startRequestId ?? "auth-request-7");
  const snapshot = options.snapshot ?? createFixtureSnapshot("ready");
  const user = userEvent.setup();
  const view = render(
    <ProvidersWorkspace
      interactionBlocked={options.interactionBlocked ?? false}
      notice={options.notice}
      onCommand={onCommand}
      onCredential={onCredential}
      onOpenNavigation={() => undefined}
      onStartAuthentication={onStartAuthentication}
      snapshot={snapshot}
    />,
  );
  return { commands, credentials, onCommand, onCredential, onStartAuthentication, snapshot, user, ...view };
}

describe("ProvidersWorkspace", () => {
  it("creates a validated router profile through the typed command boundary", async () => {
    const { commands, user } = renderWorkspace();
    await user.click(screen.getAllByRole("button", { name: "Add profile" })[0]!);
    const create = screen.getByRole("button", { name: "Create profile" });
    expect(create).toBeDisabled();

    await user.type(screen.getByRole("textbox", { name: "Profile name" }), "team-router");
    await user.selectOptions(screen.getByRole("combobox", { name: "Provider type" }), "router");
    const baseUrl = screen.getByRole("textbox", { name: "Base URL" });
    await user.type(baseUrl, "http://router.example/v1");
    expect(screen.getByText("End the base URL path with a slash.")).toBeInTheDocument();
    expect(create).toBeDisabled();

    await user.clear(baseUrl);
    await user.type(baseUrl, "https://router.example/v1/");
    await user.type(screen.getByRole("textbox", { name: "Project identity" }), "team-a");
    await user.type(screen.getByRole("textbox", { name: "Authentication header" }), "x-api-key");
    await user.click(create);

    expect(commands).toContainEqual({
      type: "upsert_provider_profile",
      profile: {
        id: "team-router",
        configuration: {
          kind: "router",
          baseUrl: "https://router.example/v1/",
          project: "team-a",
          authHeader: "x-api-key",
        },
      },
    });
  });

  it("activates, tests, edits, and duplicates one exact named profile", async () => {
    const { commands, user } = renderWorkspace();
    await user.click(screen.getByRole("button", { name: /Local router/ }));
    expect(screen.getByRole("region", { name: "Provider profile details" })).toHaveTextContent("http://127.0.0.1:11434/v1/");

    await user.click(screen.getByRole("button", { name: "Make active" }));
    await user.click(screen.getByRole("button", { name: "Test connection" }));
    expect(commands).toContainEqual({ type: "activate_provider_profile", connectionId: "local-router" });
    expect(commands).toContainEqual({ type: "test_provider_profile", connectionId: "local-router" });

    await user.click(screen.getByRole("button", { name: "Edit" }));
    expect(screen.getByRole("textbox", { name: "Profile name" })).toBeDisabled();
    const project = screen.getByRole("textbox", { name: "Project identity" });
    await user.clear(project);
    await user.type(project, "production");
    await user.click(screen.getByRole("button", { name: "Save changes" }));
    expect(commands).toContainEqual({
      type: "upsert_provider_profile",
      profile: {
        id: "local-router",
        configuration: {
          kind: "router",
          baseUrl: "http://127.0.0.1:11434/v1/",
          project: "production",
          authHeader: "authorization",
        },
      },
    });

    await user.click(screen.getByRole("button", { name: "Duplicate" }));
    const duplicateName = screen.getByRole("textbox", { name: "Profile name" });
    await user.clear(duplicateName);
    await user.type(duplicateName, "router-backup");
    await user.click(screen.getByRole("button", { name: "Duplicate profile" }));
    expect(commands).toContainEqual({
      type: "duplicate_provider_profile",
      sourceId: "local-router",
      destinationId: "router-backup",
    });
  });

  it("clears credential input before transferring a save or session-only secret", async () => {
    const { credentials, user } = renderWorkspace();
    const input = screen.getByLabelText("New provider credential");
    await user.type(input, "vault-sentinel-123");
    await user.click(screen.getByRole("button", { name: "Replace saved credential" }));
    expect(credentials).toEqual([{
      connectionId: "connection-gemini",
      operation: "replace",
      credential: "vault-sentinel-123",
    }]);
    expect(input).toHaveValue("");
    expect(document.body).not.toHaveTextContent("vault-sentinel-123");

    await user.type(input, "session-sentinel-456");
    await user.click(screen.getByRole("button", { name: "Use this session" }));
    expect(credentials[1]).toEqual({
      connectionId: "connection-gemini",
      operation: "session_only",
      credential: "session-sentinel-456",
    });
    expect(input).toHaveValue("");
  });

  it("erases an unsubmitted credential when a permission request preempts the workspace", async () => {
    const { rerender, snapshot, user } = renderWorkspace();
    const input = screen.getByLabelText("New provider credential");
    await user.type(input, "preemption-sentinel");
    expect(input).toHaveValue("preemption-sentinel");
    rerender(
      <ProvidersWorkspace
        interactionBlocked
        onCommand={async () => "committed"}
        onCredential={async () => true}
        onOpenNavigation={() => undefined}
        onStartAuthentication={async () => "auth"}
        snapshot={snapshot}
      />,
    );
    await waitFor(() => expect(input).toHaveValue(""));
    expect(document.body).not.toHaveTextContent("preemption-sentinel");
  });

  it("saves active model and reasoning defaults atomically", async () => {
    const { commands, user } = renderWorkspace();
    await user.selectOptions(screen.getByRole("combobox", { name: "Reasoning effort" }), "low");
    await user.click(screen.getByRole("button", { name: "Save defaults" }));
    expect(commands).toContainEqual({
      type: "set_provider_defaults",
      connectionId: "connection-gemini",
      modelId: "gemini/gemini-2.5-pro",
      reasoningEffort: "low",
    });
  });

  it("requires the exact profile name before permanent deletion", async () => {
    const { commands, user } = renderWorkspace();
    await user.click(screen.getByRole("button", { name: "Delete" }));
    const confirm = screen.getByRole("textbox", { name: "Confirm profile name" });
    const remove = screen.getByRole("button", { name: "Delete permanently" });
    expect(remove).toBeDisabled();
    await user.type(confirm, "connection-gemin");
    expect(remove).toBeDisabled();
    await user.type(confirm, "i");
    expect(remove).toBeEnabled();
    await user.click(remove);
    expect(commands).toContainEqual({ type: "delete_provider_profile", connectionId: "connection-gemini" });
  });

  it("uses one correlated native Codex request for start and cancellation", async () => {
    const { commands, onStartAuthentication, user } = renderWorkspace({ startRequestId: "native-auth-19" });
    await user.click(screen.getByRole("button", { name: "Connect Codex" }));
    expect(onStartAuthentication).toHaveBeenCalledTimes(1);
    expect(screen.getByRole("status", { name: /Codex sign-in in progress/ })).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Cancel sign-in" }));
    expect(commands).toContainEqual({
      type: "cancel_codex_authentication",
      authenticationRequestId: "native-auth-19",
    });
  });

  it("keeps temporary session defaults outside durable profile actions", async () => {
    const fallback: ProviderProfile = {
      id: "session:gemini",
      providerId: "gemini",
      displayName: "Gemini",
      configuration: { kind: "gemini" },
      scope: "session_default",
      active: true,
      status: "credential_required",
      credentialSource: "none",
      credentialState: "disconnected",
    };
    const base = createFixtureSnapshot("credential");
    renderWorkspace({ snapshot: { ...base, providers: [fallback], connectionId: fallback.id } });
    expect(screen.getByText("This temporary row reflects process-level defaults.", { exact: false })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Test connection" })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Edit" })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Delete" })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Save credential" })).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Use this session" })).toBeInTheDocument();
  });
});

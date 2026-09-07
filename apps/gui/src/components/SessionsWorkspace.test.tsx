import { act, cleanup, render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { ClientCommand, ClientSnapshot } from "../protocol";
import { createFixtureSnapshot } from "../transport/fixtureTransport";
import { SessionsWorkspace } from "./SessionsWorkspace";

afterEach(cleanup);

function snapshot(): ClientSnapshot {
  const value = createFixtureSnapshot("ready");
  return {
    ...value,
    sessions: value.sessions.map((session) => (
      session.id === "session-provider" ? { ...session, archived: true } : session
    )),
  };
}

function renderWorkspace(value = snapshot()) {
  const commands: ClientCommand[] = [];
  const onCommand = vi.fn(async (command: ClientCommand) => {
    commands.push(command);
    return "committed" as const;
  });
  const onOpen = vi.fn();
  const user = userEvent.setup();
  render(
    <SessionsWorkspace
      onCreate={() => undefined}
      onCommand={onCommand}
      onOpen={onOpen}
      onOpenNavigation={() => undefined}
      snapshot={value}
      timestampStyle="relative"
    />,
  );
  return { commands, onCommand, onOpen, user };
}

describe("SessionsWorkspace", () => {
  it("sorts sessions and opens a result with Enter", async () => {
    const { user, onOpen } = renderWorkspace();
    await user.selectOptions(screen.getByRole("combobox", { name: "Sort sessions" }), "title");
    const results = screen.getByRole("region", { name: "Session results" });
    const rows = within(results).getAllByRole("button");
    expect(rows[0]).toHaveTextContent("Audit context manifests");
    rows[0]!.focus();
    await user.keyboard("{Enter}");
    expect(onOpen).toHaveBeenCalledWith("session-context");
  });

  it("keeps details within the visible results and clears them for no matches", async () => {
    const { user } = renderWorkspace();
    await user.click(screen.getByRole("button", { name: "Archived 1" }));
    expect(screen.getByRole("heading", { name: "Provider recovery probes" })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Return to chat" })).not.toBeInTheDocument();
    await user.type(screen.getByRole("searchbox", { name: "Search sessions" }), "no-such-session");
    expect(screen.queryByRole("button", { name: "Delete" })).not.toBeInTheDocument();
  });

  it("waits for export settlement before opening a destructive review", async () => {
    let settle!: (outcome: "committed") => void;
    const onCommand = vi.fn(() => new Promise<"committed">((resolve) => { settle = resolve; }));
    const user = userEvent.setup();
    render(<SessionsWorkspace onCreate={() => undefined} snapshot={snapshot()} onCommand={onCommand} onOpen={() => undefined} onOpenNavigation={() => undefined} timestampStyle="relative" />);
    await user.click(screen.getByRole("button", { name: "Export Markdown" }));
    expect(screen.getByRole("button", { name: "Delete" })).toBeDisabled();
    await user.click(screen.getByRole("button", { name: "Delete" }));
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    await act(async () => settle("committed"));
    await user.click(screen.getByRole("button", { name: "Delete" }));
    const confirmation = screen.getByRole("textbox", { name: "Confirm session title" });
    await user.type(confirmation, "Design the GUI migration");
    expect(confirmation).toHaveValue("Design the GUI migration");
    expect(screen.getByRole("button", { name: "Delete permanently" })).toBeEnabled();
  });

  it("searches identities, filters archives, and opens a selected session", async () => {
    const { onOpen, user } = renderWorkspace();
    const search = screen.getByRole("searchbox", { name: "Search sessions" });
    await user.type(search, "session-context");
    expect(screen.getByRole("button", { name: /Audit context manifests/ })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /Design the GUI migration/ })).not.toBeInTheDocument();

    await user.clear(search);
    await user.click(screen.getByRole("button", { name: "Archived 1" }));
    expect(screen.getByRole("button", { name: /Provider recovery probes/ })).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Open 2" }));
    await user.click(screen.getByRole("button", { name: /Audit context manifests/ }));
    await user.click(screen.getByRole("button", { name: "Open session" }));
    expect(onOpen).toHaveBeenCalledWith("session-context");
  });

  it("renames and archives one exact selected session", async () => {
    const { commands, user } = renderWorkspace();
    await user.click(screen.getByRole("button", { name: /Audit context manifests/ }));
    await user.click(screen.getByRole("button", { name: "Rename" }));
    const title = screen.getByRole("textbox", { name: "New title" });
    await user.clear(title);
    await user.type(title, "Context manifest review");
    await user.click(screen.getByRole("button", { name: "Save title" }));
    expect(commands).toContainEqual({
      type: "rename_session",
      sessionId: "session-context",
      title: "Context manifest review",
    });

    await user.click(screen.getByRole("button", { name: "Archive" }));
    expect(screen.getByRole("dialog", { name: "Archive “Audit context manifests”?" })).toHaveTextContent("session-context");
    await user.click(screen.getByRole("button", { name: "Archive this session" }));
    expect(commands).toContainEqual({ type: "archive_session", sessionId: "session-context" });
  });

  it("requires the exact title before permanent deletion", async () => {
    const { commands, user } = renderWorkspace();
    await user.click(screen.getByRole("button", { name: /Audit context manifests/ }));
    await user.click(screen.getByRole("button", { name: "Delete" }));
    const confirm = screen.getByRole("textbox", { name: "Confirm session title" });
    const remove = screen.getByRole("button", { name: "Delete permanently" });
    expect(remove).toBeDisabled();
    await user.type(confirm, "Audit context manifest");
    expect(remove).toBeDisabled();
    await user.type(confirm, "s");
    expect(remove).toBeEnabled();
    await user.click(remove);
    expect(commands).toContainEqual({ type: "delete_session", sessionId: "session-context" });
  });

  it("exports an exact inactive or archived session and restores an archive", async () => {
    const { commands, user } = renderWorkspace();
    await user.click(screen.getByRole("button", { name: "Export Markdown" }));
    expect(commands).toContainEqual({ type: "export_transcript", sessionId: "session-gui-migration" });

    await user.click(screen.getByRole("button", { name: "Archived 1" }));
    await user.click(screen.getByRole("button", { name: /Provider recovery probes/ }));
    await user.click(screen.getByRole("button", { name: "Export Markdown" }));
    expect(commands).toContainEqual({ type: "export_transcript", sessionId: "session-provider" });
    await user.click(screen.getByRole("button", { name: "Restore session" }));
    expect(commands).toContainEqual({ type: "unarchive_session", sessionId: "session-provider" });
  });
});

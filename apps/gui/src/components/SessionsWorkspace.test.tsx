import { cleanup, render, screen } from "@testing-library/react";
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

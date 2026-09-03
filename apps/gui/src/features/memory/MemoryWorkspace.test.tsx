import { useState } from "react";
import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { ClientCommand } from "../../protocol";
import { FixtureMemory, fixtureMemoryRow } from "./fixtures";
import { emptyMemory, type MemoryProjection } from "./model";
import { MemoryWorkspace } from "./MemoryWorkspace";

afterEach(cleanup);

function setup() {
  const host = new FixtureMemory();
  const commands: ClientCommand[] = [];
  const onDialogChange = vi.fn();
  function Harness() {
    const [memory, setMemory] = useState(host.page());
    return <MemoryWorkspace memory={memory} blocked={false} onOpenNavigation={() => undefined} onDialogChange={onDialogChange} onCommand={async (command) => {
      commands.push(structuredClone(command));
      if (command.type === "memory") setMemory(structuredClone(host.command(command.command)));
      return "committed";
    }} />;
  }
  render(<Harness />);
  return { commands, onDialogChange, user: userEvent.setup() };
}

describe("Memory workspace", () => {
  it("reviews an imported proposal before issuing a distinct exact approval", async () => {
    const { commands, user, onDialogChange } = setup();
    await user.click(await screen.findByRole("button", { name: /proposed workspace/ }));
    expect(screen.getByText("Untrusted source")).toBeInTheDocument();
    expect(screen.getByText(/cannot authorize itself/)).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Review approval" }));
    const dialog = screen.getByRole("dialog", { name: "Approve proposal" });
    expect(within(dialog).getByText("memory-2-revision-1")).toBeInTheDocument();
    expect(commands.filter((command) => command.type === "memory" && command.command.kind === "approve")).toHaveLength(0);
    expect(onDialogChange).toHaveBeenCalledWith(true);
    await user.click(within(dialog).getByRole("button", { name: "Approve proposal" }));
    expect(commands).toContainEqual({ type: "memory", command: { kind: "approve", payload: { memory_id: "memory-2", expected_last_sequence: "3", proposal_revision_id: "memory-2-revision-1" } } });
    await waitFor(() => expect(screen.queryByRole("dialog")).not.toBeInTheDocument());
    expect(screen.getByText("workspace memory · Revision 2")).toBeInTheDocument();
    expect(screen.queryByText("Untrusted source")).not.toBeInTheDocument();
  });

  it("corrects with an inert comparison, exports, retracts, and confirms deletion", async () => {
    const { commands, user } = setup();
    await user.click(await screen.findByRole("button", { name: "Correct" }));
    const editor = screen.getByRole("textbox", { name: "Memory content" });
    fireEvent.change(editor, { target: { value: '<img src=x onerror="alert(1)"> revised preference' } });
    expect(screen.getByLabelText("Proposed content comparison").querySelector("img")).toBeNull();
    await user.click(screen.getByRole("button", { name: "Correct memory" }));
    await waitFor(() => expect(screen.queryByRole("dialog")).not.toBeInTheDocument());
    await user.click(screen.getByRole("button", { name: "Export" }));
    await user.click(screen.getByRole("button", { name: "Export memory" }));
    await waitFor(() => expect(screen.queryByRole("dialog")).not.toBeInTheDocument());
    await user.click(screen.getByRole("button", { name: "Retract" }));
    await user.click(screen.getByRole("button", { name: "Retract memory" }));
    await waitFor(() => expect(screen.queryByRole("dialog")).not.toBeInTheDocument());
    expect(screen.queryByRole("button", { name: "Correct" })).not.toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Delete content" }));
    const remove = screen.getByRole("button", { name: "Delete memory content" });
    expect(remove).toBeDisabled();
    await user.type(screen.getByRole("textbox", { name: "Confirm memory identity" }), "memory-1");
    await user.click(remove);
    await waitFor(() => expect(screen.queryByRole("dialog")).not.toBeInTheDocument());
    expect(screen.getByText("Content erased. Only audit metadata remains.")).toBeInTheDocument();
    expect(commands.filter((command) => command.type === "memory").map((command) => command.command.kind)).toEqual(expect.arrayContaining(["revise", "export", "retract", "delete"]));
  });

  it("sends literal search and filters to the host, including empty results", async () => {
    const { commands, user } = setup();
    await screen.findByRole("button", { name: "Correct" });
    await user.type(screen.getByRole("searchbox", { name: "Search memory" }), 'no-match OR "literal"');
    await user.click(screen.getByRole("button", { name: "Search" }));
    expect(await screen.findByText("No matching memory")).toBeInTheDocument();
    await user.selectOptions(screen.getByRole("combobox", { name: "Memory status" }), "inactive");
    expect(commands).toContainEqual(expect.objectContaining({ type: "memory", command: { kind: "query", payload: expect.objectContaining({ literal: 'no-match OR "literal"', status: "inactive", before: null }) } }));
  });

  it("uses opaque paging boundaries and resets them when scope changes", async () => {
    const commands: ClientCommand[] = [];
    function Harness() {
      const [memory, setMemory] = useState<MemoryProjection>(emptyMemory());
      return <MemoryWorkspace memory={memory} blocked={false} onOpenNavigation={() => undefined} onDialogChange={() => undefined} onCommand={async (command) => {
        commands.push(command);
        if (command.type === "memory" && command.command.kind === "query") {
          const query = command.command.payload;
          setMemory({ ...emptyMemory(), view_generation: query.view_generation, rows: [fixtureMemoryRow(query.before ? 2 : 1)], total: 1, next_cursor: query.before ? null : "opaque-boundary" });
        }
        return "committed";
      }} />;
    }
    const user = userEvent.setup(); render(<Harness />);
    await user.click(await screen.findByRole("button", { name: "Next" }));
    expect(await screen.findByText("Page 2")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Next" })).toBeDisabled();
    await user.click(screen.getByRole("button", { name: "Previous" }));
    expect(await screen.findByText("Page 1")).toBeInTheDocument();
    await user.selectOptions(screen.getByRole("combobox", { name: "Memory scope" }), "session");
    expect(commands.at(-1)).toMatchObject({ command: { payload: { scope: "session", before: null, direction: "first" } } });
  });

  it("invalidates a stale review and yields immediately to permissions", async () => {
    const row = fixtureMemoryRow(1);
    const memory = { ...emptyMemory(), rows: [row], total: 1, view_generation: "1" };
    const props = { blocked: false, onOpenNavigation: vi.fn(), onDialogChange: vi.fn(), onCommand: vi.fn(async () => "committed" as const) };
    const view = render(<MemoryWorkspace {...props} memory={{ ...memory, view_generation: "0" }} />);
    view.rerender(<MemoryWorkspace {...props} memory={memory} />);
    const user = userEvent.setup();
    await user.click(screen.getByRole("button", { name: "Correct" }));
    const changed = structuredClone(memory);
    changed.rows[0]!.detail!.revision_context!.expected_last_sequence = "99";
    view.rerender(<MemoryWorkspace {...props} memory={changed} />);
    expect(screen.getByText(/changed while you were reviewing/)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Correct memory" })).toBeDisabled();
    view.rerender(<MemoryWorkspace {...props} blocked memory={changed} />);
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  });
});

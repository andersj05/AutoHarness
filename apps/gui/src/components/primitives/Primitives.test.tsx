import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { useState } from "react";
import { afterEach, describe, expect, it } from "vitest";
import {
  Button,
  Callout,
  Chip,
  CommandPalette,
  Dialog,
  Field,
  Menu,
  Meter,
  SplitPane,
  StatusSurface,
  ToolCard,
  VirtualList,
} from ".";

afterEach(cleanup);

describe("desktop design-system primitives", () => {
  it("exposes loading buttons and field errors without color-only state", () => {
    render(
      <>
        <Button loading loadingLabel="Saving settings" variant="primary">Save</Button>
        <Field error="A model is required" label="Default model" />
        <Chip intent="danger">Disconnected</Chip>
      </>,
    );
    expect(screen.getByRole("button", { name: "Saving settings" })).toBeDisabled();
    const field = screen.getByRole("textbox", { name: "Default model" });
    expect(field).toHaveAttribute("aria-invalid", "true");
    expect(field).toHaveAccessibleDescription("A model is required");
    expect(screen.getByText("Disconnected").closest(".dsChip")).toHaveAttribute("data-intent", "danger");
  });

  it("moves menu focus with arrows and activates the focused item", async () => {
    const user = userEvent.setup();
    const activated: string[] = [];
    render(
      <Menu
        ariaLabel="Workspace actions"
        items={[
          { id: "chat", label: "Open chat" },
          { id: "settings", label: "Open settings" },
        ]}
        onAction={(id) => activated.push(id)}
      />,
    );
    const first = screen.getByRole("menuitem", { name: "Open chat" });
    first.focus();
    await user.keyboard("{ArrowDown}{Enter}");
    expect(screen.getByRole("menuitem", { name: "Open settings" })).toHaveFocus();
    expect(activated).toEqual(["settings"]);
  });

  it("traps dialog focus and restores the invoking control", async () => {
    const user = userEvent.setup();
    function Harness() {
      const [open, setOpen] = useState(false);
      return (
        <>
          <button onClick={() => setOpen(true)} type="button">Open review</button>
          {open ? (
            <Dialog footer={<button type="button">Last action</button>} onClose={() => setOpen(false)} title="Review operation">
              <button data-initial-focus type="button">First action</button>
            </Dialog>
          ) : null}
        </>
      );
    }
    render(<Harness />);
    const trigger = screen.getByRole("button", { name: "Open review" });
    await user.click(trigger);
    expect(screen.getByRole("button", { name: "First action" })).toHaveFocus();
    screen.getByRole("button", { name: "Close dialog" }).focus();
    await user.keyboard("{Shift>}{Tab}{/Shift}");
    expect(screen.getByRole("button", { name: "Last action" })).toHaveFocus();
    await user.keyboard("{Escape}");
    expect(trigger).toHaveFocus();
  });

  it("filters and chooses command-palette actions from the keyboard", async () => {
    const user = userEvent.setup();
    const selected: string[] = [];
    render(
      <CommandPalette
        items={[
          { id: "session", label: "New session", keywords: "create" },
          { id: "settings", label: "Open settings", keywords: "preferences" },
        ]}
        onClose={() => undefined}
        onSelect={(id) => selected.push(id)}
      />,
    );
    const search = screen.getByRole("searchbox", { name: "Search commands" });
    await user.type(search, "pref");
    expect(screen.queryByRole("menuitem", { name: "New session" })).not.toBeInTheDocument();
    await user.keyboard("{ArrowDown}{Enter}");
    expect(selected).toEqual(["settings"]);
  });

  it("resizes split panes through an accessible separator", () => {
    const changes: number[] = [];
    render(<SplitPane label="Resize inspector" onValueChange={(value) => changes.push(value)} secondary={<div>Inspector</div>} value={60}><div>Workspace</div></SplitPane>);
    const separator = screen.getByRole("separator", { name: "Resize inspector" });
    fireEvent.keyDown(separator, { key: "ArrowRight" });
    fireEvent.keyDown(separator, { key: "End" });
    expect(changes).toEqual([62, 80]);
  });

  it("renders only a bounded virtual-list window with complete set metadata", () => {
    const items = Array.from({ length: 1000 }, (_, index) => `Session ${index}`);
    render(<VirtualList ariaLabel="Sessions" height={120} itemKey={(item) => item} items={items} overscan={1} renderItem={(item) => item} rowHeight={30} />);
    const list = screen.getByRole("list", { name: "Sessions" });
    expect(screen.getAllByRole("listitem")).toHaveLength(6);
    expect(screen.getAllByRole("listitem")[0]).toHaveAttribute("aria-setsize", "1000");
    fireEvent.scroll(list, { target: { scrollTop: 600 } });
    expect(screen.getByText("Session 19")).toBeInTheDocument();
    expect(screen.queryByText("Session 0")).not.toBeInTheDocument();
  });

  it("uses native meter semantics and text-labelled status redundancy", () => {
    render(
      <>
        <Meter detail="50 of 200 tokens" label="Context used" max={200} value={50} />
        <Callout detail="Reconnect before sending." intent="warning" title="Provider offline" />
        <StatusSurface detail="The turn can be retried." intent="danger" title="Response failed" />
        <ToolCard name="read_file" resource="docs/README.md" status="succeeded" summary="Read one file">Complete</ToolCard>
      </>,
    );
    expect(screen.getByRole("progressbar", { name: "Context used" })).toHaveValue(50);
    expect(screen.getByRole("status", { name: "warning: Provider offline" })).toHaveTextContent("Provider offline");
    expect(screen.getByRole("alert", { name: "danger: Response failed" })).toHaveTextContent("Response failed");
    expect(screen.getByText("succeeded")).toBeInTheDocument();
  });
});

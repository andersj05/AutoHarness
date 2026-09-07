import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { ActiveSessionProjection, ModelDescriptor } from "../protocol";
import { Conversation } from "./Conversation";

const model: ModelDescriptor = {
  id: "fixture/model",
  displayName: "Fixture Model",
  provider: "Browser fixture",
  description: "Simulated model",
  selectable: true,
};

function session(content: string): ActiveSessionProjection {
  return {
    id: "session-1",
    revision: "1",
    title: "Tail follow",
    selectedModelId: model.id,
    workspaceLabel: "Fixture",
    transcript: [{ kind: "message", id: "message-1", role: "agent", content }],
    attempt: { kind: "idle" },
  };
}

const callbacks = {
  draft: "",
  submissionBehavior: "enter" as const,
  timestampStyle: "relative" as const,
  onCancel: vi.fn(),
  onDraftChange: vi.fn(),
  onOpenCredential: vi.fn(),
  onOpenInspector: vi.fn(),
  onOpenModelPicker: vi.fn(),
  onOpenNavigation: vi.fn(),
  onExport: vi.fn(async () => "committed" as const),
  onRefresh: vi.fn(),
  onRetry: vi.fn(),
  onSubmit: vi.fn(async () => "committed" as const),
};

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
});

describe("Conversation tail following", () => {
  it("starts at the tail, follows nearby updates, and respects manual upward scrolling", () => {
    vi.spyOn(HTMLElement.prototype, "scrollHeight", "get").mockReturnValue(1_000);
    vi.spyOn(HTMLElement.prototype, "clientHeight", "get").mockReturnValue(400);
    const view = render(
      <Conversation
        {...callbacks}
        catalog={{ status: "ready", source: "live", models: [model] }}
        connection={{ kind: "online", providerLabel: "Fixture", credentialSource: "Simulated" }}
        model={model}
        runtimeMode="fixture"
        session={session("initial")}
      />,
    );
    const scroller = view.container.querySelector<HTMLElement>(".conversationScroll")!;
    expect(scroller.scrollTop).toBe(1_000);

    scroller.scrollTop = 100;
    fireEvent.scroll(scroller);
    view.rerender(
      <Conversation
        {...callbacks}
        catalog={{ status: "ready", source: "live", models: [model] }}
        connection={{ kind: "online", providerLabel: "Fixture", credentialSource: "Simulated" }}
        model={model}
        runtimeMode="fixture"
        session={session("initial plus a streamed update")}
      />,
    );
    expect(scroller.scrollTop).toBe(100);

    scroller.scrollTop = 600;
    fireEvent.scroll(scroller);
    view.rerender(
      <Conversation
        {...callbacks}
        catalog={{ status: "ready", source: "live", models: [model] }}
        connection={{ kind: "online", providerLabel: "Fixture", credentialSource: "Simulated" }}
        model={model}
        runtimeMode="fixture"
        session={session("initial plus a streamed update and completion")}
      />,
    );
    expect(scroller.scrollTop).toBe(1_000);
  });

  it("searches transcript content and discloses a matching tool", async () => {
    const user = userEvent.setup();
    const searchable = session("The first durable message");
    searchable.transcript = [
      ...searchable.transcript,
      {
        kind: "tool",
        id: "tool-1",
        name: "workspace.read",
        summary: "Read the implementation plan",
        resource: "docs/design/GUI_IMPLEMENTATION_PLAN.md",
        status: "succeeded",
        detail: "Stage 4 loaded",
      },
      { kind: "message", id: "message-2", role: "agent", content: "The second durable message" },
    ];
    render(
      <Conversation
        {...callbacks}
        catalog={{ status: "ready", source: "live", models: [model] }}
        connection={{ kind: "online", providerLabel: "Fixture", credentialSource: "Simulated" }}
        model={model}
        runtimeMode="fixture"
        session={searchable}
      />,
    );

    await user.keyboard("{Control>}f{/Control}");
    const search = screen.getByRole("searchbox", { name: "Find in transcript" });
    expect(search).toHaveFocus();
    await user.type(search, "durable message");
    expect(screen.getByText("1 / 2")).toBeInTheDocument();
    expect(document.querySelector("mark")).toHaveTextContent("durable message");
    await user.clear(search);
    await user.type(search, "GUI_IMPLEMENTATION_PLAN");
    expect(document.querySelector(".toolCard")).toHaveAttribute("open");
  });

  it("copies a complete plain-text transcript and requests host export", async () => {
    const user = userEvent.setup();
    const writeText = vi.spyOn(navigator.clipboard, "writeText").mockResolvedValue();
    const onExport = vi.fn(async () => "committed" as const);
    render(
      <Conversation
        {...callbacks}
        catalog={{ status: "ready", source: "live", models: [model] }}
        connection={{ kind: "online", providerLabel: "Fixture", credentialSource: "Simulated" }}
        model={model}
        onExport={onExport}
        runtimeMode="fixture"
        session={session("Copy this exact response")}
      />,
    );

    await user.click(screen.getByRole("button", { name: "Copy transcript" }));
    expect(writeText).toHaveBeenCalledWith("AutoHarness:\nCopy this exact response");
    expect(screen.getByText("Transcript copied to the clipboard.")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Export transcript" }));
    expect(onExport).toHaveBeenCalledTimes(1);
  });
});

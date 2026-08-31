import { fireEvent, render } from "@testing-library/react";
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
  onCancel: vi.fn(),
  onDraftChange: vi.fn(),
  onOpenCredential: vi.fn(),
  onOpenInspector: vi.fn(),
  onOpenModelPicker: vi.fn(),
  onOpenNavigation: vi.fn(),
  onRefresh: vi.fn(),
  onRetry: vi.fn(),
  onSubmit: vi.fn(async () => "committed" as const),
};

afterEach(() => {
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
});

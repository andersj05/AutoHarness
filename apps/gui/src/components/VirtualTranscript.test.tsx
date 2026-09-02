import { createRef } from "react";
import { cleanup, render } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import type { TextMessage } from "../protocol";
import { VirtualTranscript } from "./VirtualTranscript";

afterEach(cleanup);

function messages(count: number): TextMessage[] {
  return Array.from({ length: count }, (_, index) => ({
    kind: "message" as const,
    id: `message-${index}`,
    role: index % 2 === 0 ? "user" as const : "agent" as const,
    content: `Transcript item ${index}`,
  }));
}

describe("VirtualTranscript", () => {
  it("keeps the rendered DOM bounded independently of transcript length", () => {
    const scrollRef = createRef<HTMLDivElement>();
    const items = messages(65_000);
    const { container } = render(
      <div ref={scrollRef}>
        <VirtualTranscript
          items={items}
          renderItem={(item) => <p>{item.kind === "message" ? item.content : item.name}</p>}
          scrollRef={scrollRef}
          sessionId="long-session"
        />
      </div>,
    );

    expect(container.querySelectorAll("[data-virtual-transcript-item]")).toHaveLength(36);
    expect(container).toHaveTextContent("Transcript item 64999");
    expect(container).not.toHaveTextContent("Transcript item 0");
  });

  it("moves its bounded window to an exact search result", () => {
    const scrollRef = createRef<HTMLDivElement>();
    const items = messages(5_000);
    const { container, rerender } = render(
      <div ref={scrollRef}>
        <VirtualTranscript items={items} renderItem={(item) => <p>{item.kind === "message" ? item.content : item.name}</p>} scrollRef={scrollRef} sessionId="search-session" />
      </div>,
    );
    rerender(
      <div ref={scrollRef}>
        <VirtualTranscript activeIndex={42} items={items} renderItem={(item) => <p>{item.kind === "message" ? item.content : item.name}</p>} scrollRef={scrollRef} sessionId="search-session" />
      </div>,
    );

    expect(container.querySelector('[data-transcript-index="42"]')).toHaveTextContent("Transcript item 42");
    expect(container.querySelectorAll("[data-virtual-transcript-item]").length).toBeLessThanOrEqual(36);
  });
});

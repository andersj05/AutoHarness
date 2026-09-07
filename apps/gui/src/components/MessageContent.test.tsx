import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import { MessageContent } from "./MessageContent";

afterEach(cleanup);

describe("MessageContent", () => {
  it("renders useful Markdown structure without promoting headings to page titles", () => {
    const { container } = render(<MessageContent text={'# Plan\n\n1. **Read** the code\n2. Run `cargo test`\n\n```rust\nfn main() {}\n```\n\n| Task | State |\n| --- | --- |\n| Review | Ready |'} />);
    expect(screen.getByRole("heading", { name: "Plan", level: 3 })).toBeInTheDocument();
    expect(screen.getAllByRole("listitem")).toHaveLength(2);
    expect(screen.getByRole("table")).toHaveTextContent("Review");
    expect(container.querySelector("pre code")).toHaveTextContent("fn main() {}");
  });

  it("keeps HTML, images, navigation, and executable protocols inert", () => {
    const { container } = render(<MessageContent text={'<script>alert(1)</script>\n\n<img src="https://example.com/tracker" onerror="alert(1)">\n\n[Run](javascript:alert) [File](file:///secret) [Web](https://example.com)\n\n![Remote image](https://example.com/pixel)'} />);
    expect(container.querySelectorAll("script, img, a, iframe, object")).toHaveLength(0);
    expect(container).toHaveTextContent("https://example.com");
    expect(container).toHaveTextContent("[Image: Remote image]");
  });

  it("highlights literal matches and preserves plain user input", () => {
    const { rerender, container } = render(<MessageContent text="**literal**\nnext" plain />);
    expect(container).toHaveTextContent("**literal**");
    rerender(<MessageContent text="First TEST, then test." highlight="test" plain />);
    expect(container.querySelectorAll("mark")).toHaveLength(2);
  });
});

import { cleanup, render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";
import { HelpWorkspace } from "./HelpWorkspace";

afterEach(cleanup);
describe("Help reader", () => {
  it("opens relevant workspaces and recovers a search with no matching topics", async () => {
    const user = userEvent.setup();
    const onRoute = vi.fn();
    render(<HelpWorkspace onOpenNavigation={vi.fn()} onRoute={onRoute} />);
    await user.click(screen.getByRole("button", { name: "Open Providers" }));
    expect(onRoute).toHaveBeenCalledWith("providers");
    await user.click(within(screen.getByRole("navigation", { name: "Help topics" })).getByRole("button", { name: "Memory and trust" }));
    expect(screen.getByRole("article")).toHaveTextContent("untrusted");
    await user.type(screen.getByRole("searchbox", { name: "Search help" }), "no-such-topic");
    expect(screen.getByRole("heading", { name: "No matching topics" })).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Clear search" }));
    expect(screen.getByRole("heading", { name: "Memory and trust" })).toBeInTheDocument();
    await user.selectOptions(screen.getByRole("combobox", { name: "Help topic" }), "shortcuts");
    expect(screen.getByRole("heading", { name: "Keyboard shortcuts" })).toBeInTheDocument();
  });
});

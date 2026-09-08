import { cleanup, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";
import { createFixtureSnapshot } from "../transport/fixtureTransport";
import { ContextInspector } from "./ContextInspector";

afterEach(cleanup);
const snapshot = createFixtureSnapshot("ready");
const props = { activity: [], connection: snapshot.connection, mobileOpen: true, runtimeMode: "fixture" as const, onClose: vi.fn(), onChangeModel: vi.fn(), model: snapshot.catalog.models[0] };

describe("Session details", () => {
  it("shows exact reported counts, keeps partial usage unknown, and opens the model picker", async () => {
    const user = userEvent.setup();
    render(<ContextInspector {...props} session={{ ...snapshot.activeSession!, attempt: { kind: "completed", id: "turn", inputTokens: "9007199254740993" } }} />);
    expect(screen.getByText(new Intl.NumberFormat().format(9007199254740993n))).toBeInTheDocument();
    expect(screen.getByText("Not reported")).toBeInTheDocument();
    expect(screen.queryByRole("progressbar")).not.toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Change session model" }));
    expect(props.onChangeModel).toHaveBeenCalledOnce();
  });

  it("does not allow a model change during a response and labels non-online states correctly", () => {
    render(<ContextInspector {...props} connection={{ kind: "credential_required", providerLabel: "Gemini", reason: "Connect first" }} session={{ ...snapshot.activeSession!, attempt: { kind: "streaming", id: "turn", startedAt: "2026-09-07T00:00:00Z" } }} />);
    expect(screen.getByRole("button", { name: "Change session model" })).toBeDisabled();
    expect(screen.getByText("Sign-in needed")).toBeInTheDocument();
    expect(screen.getByText("Token counts appear when the response finishes.")).toBeInTheDocument();
    expect(screen.queryByText("Not reported")).not.toBeInTheDocument();
  });
});

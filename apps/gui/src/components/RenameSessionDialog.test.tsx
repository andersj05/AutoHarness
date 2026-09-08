import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";
import { RenameSessionDialog } from "./RenameSessionDialog";

afterEach(cleanup);
describe("Rename session", () => {
  it("validates UTF-8 size and keeps rejected edits available", async () => {
    const user = userEvent.setup();
    const onCommand = vi.fn(async () => "rejected" as const);
    const onClose = vi.fn();
    render(<RenameSessionDialog session={{ id: "exact-session", title: "Original" }} onCommand={onCommand} onClose={onClose} />);
    const field = screen.getByRole("textbox", { name: "New title" });
    fireEvent.change(field, { target: { value: "語".repeat(43) } });
    expect(screen.getByRole("button", { name: "Save title" })).toBeDisabled();
    fireEvent.change(field, { target: { value: "New title" } });
    await user.keyboard("{Enter}");
    expect(onCommand).toHaveBeenCalledWith({ type: "rename_session", sessionId: "exact-session", title: "New title" });
    expect(onClose).not.toHaveBeenCalled();
    expect(field).toHaveValue("New title");
    expect(screen.getByRole("status")).toHaveTextContent("Could not rename");
  });
});

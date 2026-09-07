import { cleanup, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, expect, it, vi } from "vitest";
import { ModelPicker } from "./ModelPicker";

afterEach(cleanup);

it("navigates available models with the keyboard without selecting unavailable models", async () => {
  const user = userEvent.setup();
  const onSelect = vi.fn();
  render(<ModelPicker models={[
    { id: "one", displayName: "One", provider: "Preview", description: "", selectable: true },
    { id: "two", displayName: "Two", provider: "Preview", description: "", selectable: false },
    { id: "three", displayName: "Three", provider: "Preview", description: "", selectable: true },
  ]} onClose={() => undefined} onRefresh={() => undefined} onSelect={onSelect} />);
  await user.keyboard("{ArrowDown}");
  expect(screen.getByRole("radio", { name: /^One/ })).toHaveFocus();
  await user.keyboard("{ArrowDown}{Enter}");
  expect(onSelect).toHaveBeenCalledExactlyOnceWith("three");
  await user.click(screen.getByRole("searchbox", { name: "Search models" }));
  await user.type(screen.getByRole("searchbox", { name: "Search models" }), "Two{Enter}");
  expect(onSelect).toHaveBeenCalledTimes(1);
});

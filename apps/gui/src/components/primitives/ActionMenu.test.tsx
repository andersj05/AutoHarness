import { cleanup, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, expect, it, vi } from "vitest";
import { ActionMenu } from "./ActionMenu";

afterEach(cleanup);

it("supports keyboard actions, dismissal, focus restoration, and permission preemption", async () => {
  const user = userEvent.setup();
  const onAction = vi.fn();
  const props = { label: "Session actions", items: [{ id: "copy", label: "Copy" }, { id: "export", label: "Export" }], onAction };
  const { rerender } = render(<><ActionMenu {...props} /><button>Outside</button></>);
  const trigger = screen.getByRole("button", { name: "Session actions" });
  await user.click(trigger);
  expect(screen.getByRole("menuitem", { name: "Copy" })).toHaveFocus();
  await user.keyboard("{ArrowDown}{Enter}");
  expect(onAction).toHaveBeenCalledExactlyOnceWith("export");
  expect(trigger).toHaveFocus();
  await user.click(trigger);
  await user.keyboard("{Escape}");
  expect(trigger).toHaveFocus();
  expect(screen.queryByRole("menu")).not.toBeInTheDocument();
  await user.click(trigger);
  await user.click(screen.getByRole("button", { name: "Outside" }));
  expect(screen.queryByRole("menu")).not.toBeInTheDocument();
  await user.click(trigger);
  rerender(<ActionMenu {...props} blocked />);
  expect(screen.queryByRole("menu")).not.toBeInTheDocument();
});

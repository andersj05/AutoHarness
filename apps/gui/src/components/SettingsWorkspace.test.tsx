import { cleanup, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { ClientCommand, ClientSettingsProjection } from "../protocol";
import { createFixtureSnapshot } from "../transport/fixtureTransport";
import { SettingsWorkspace } from "./SettingsWorkspace";

afterEach(cleanup);

function renderWorkspace(settings: ClientSettingsProjection = createFixtureSnapshot("ready").settings) {
  const commands: ClientCommand[] = [];
  const onCommand = vi.fn(async (command: ClientCommand) => {
    commands.push(command);
    return "committed" as const;
  });
  const user = userEvent.setup();
  render(<SettingsWorkspace onCommand={onCommand} onOpenNavigation={() => undefined} settings={settings} />);
  return { commands, onCommand, user };
}

describe("SettingsWorkspace", () => {
  it("finds option names across categories and returns to the selected category", async () => {
    const { user } = renderWorkspace();
    const search = screen.getByRole("searchbox", { name: "Search settings" });
    await user.type(search, "200");
    expect(screen.getAllByRole("combobox")).toHaveLength(1);
    expect(screen.getByRole("combobox", { name: "Zoom" })).toBeVisible();
    await user.click(screen.getByRole("button", { name: "Conversation" }));
    expect(search).toHaveValue("");
    expect(screen.getByRole("button", { name: "Conversation" })).toHaveAttribute("aria-pressed", "true");
    expect(screen.getByRole("combobox", { name: "Send with" })).toBeVisible();
    expect(screen.queryByRole("combobox", { name: "Zoom" })).not.toBeInTheDocument();
  });

  it("inspects and explains every renderer preference with its effective source", () => {
    const settings = structuredClone(createFixtureSnapshot("ready").settings);
    settings.themePreset = { value: "ocean", source: "workspace_file", userOverride: false };
    settings.colorMode = { value: "high-contrast", source: "environment", userOverride: true };
    renderWorkspace(settings);

    expect(screen.getAllByRole("combobox")).toHaveLength(1);
    expect(screen.getByRole("radiogroup", { name: "Theme" })).toBeVisible();
    expect(screen.getByRole("radio", { name: "Ocean" })).toBeChecked();
    expect(screen.queryByRole("checkbox", { name: "Reduce motion" })).not.toBeInTheDocument();
    expect(screen.getByText("workspace settings")).toBeInTheDocument();
    expect(screen.getByTitle("The current workspace supplies this value.")).toBeInTheDocument();
    expect(screen.getByTitle("An environment variable currently has precedence.")).toBeInTheDocument();
    expect(screen.getByText("Your saved value is currently overridden.")).toBeInTheDocument();
    expect(screen.getByRole("navigation", { name: "Settings sections" })).toBeInTheDocument();
  });

  it("issues a typed host command for every setting and a null reset", async () => {
    const { commands, user } = renderWorkspace();

    await user.click(screen.getByRole("radio", { name: "Rose" }));
    await user.selectOptions(screen.getByRole("combobox", { name: "Color and contrast" }), "no-color");
    await user.click(screen.getByRole("button", { name: "Accessibility" }));
    await user.selectOptions(screen.getByRole("combobox", { name: "Zoom" }), "150");
    await user.selectOptions(screen.getByRole("combobox", { name: "Text size" }), "large");
    await user.click(screen.getByRole("checkbox", { name: "Reduce motion" }));
    await user.click(screen.getByRole("button", { name: "Conversation" }));
    await user.selectOptions(screen.getByRole("combobox", { name: "Timestamps" }), "absolute");
    await user.selectOptions(screen.getByRole("combobox", { name: "Send with" }), "control_s");
    await user.click(screen.getByRole("button", { name: "Reset Send with to its inherited value" }));

    await waitFor(() => expect(commands).toHaveLength(8));
    expect(commands).toEqual([
      { type: "update_client_preference", change: { kind: "theme_preset", value: "rose" } },
      { type: "update_client_preference", change: { kind: "color_mode", value: "no-color" } },
      { type: "update_client_preference", change: { kind: "zoom_percent", value: 150 } },
      { type: "update_client_preference", change: { kind: "font_size", value: "large" } },
      { type: "update_client_preference", change: { kind: "reduced_motion", value: true } },
      { type: "update_client_preference", change: { kind: "timestamp_style", value: "absolute" } },
      { type: "update_client_preference", change: { kind: "composer_submit_behavior", value: "control_s" } },
      { type: "update_client_preference", change: { kind: "composer_submit_behavior", value: null } },
    ]);
    expect(screen.getByText("Submission behavior updated.")).toBeInTheDocument();
  });

  it("filters settings by topic without removing the section navigation landmark", async () => {
    const { user } = renderWorkspace();
    await user.type(screen.getByRole("searchbox", { name: "Search settings" }), "zoom");
    expect(screen.getByRole("heading", { name: "Accessibility" })).toBeInTheDocument();
    expect(screen.getAllByRole("combobox")).toHaveLength(1);
    expect(screen.queryByRole("checkbox")).not.toBeInTheDocument();
    expect(screen.queryByRole("heading", { name: "Appearance" })).not.toBeInTheDocument();
    expect(screen.getByRole("navigation", { name: "Settings sections" })).toBeInTheDocument();

    await user.clear(screen.getByRole("searchbox", { name: "Search settings" }));
    await user.type(screen.getByRole("searchbox", { name: "Search settings" }), "unrelated");
    expect(screen.getByRole("heading", { name: /No settings match/ })).toBeInTheDocument();
  });
});

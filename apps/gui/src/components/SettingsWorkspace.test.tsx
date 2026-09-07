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
  it("inspects and explains every renderer preference with its effective source", () => {
    const settings = structuredClone(createFixtureSnapshot("ready").settings);
    settings.themePreset = { value: "ocean", source: "workspace_file", userOverride: false };
    settings.colorMode = { value: "high-contrast", source: "environment", userOverride: true };
    renderWorkspace(settings);

    expect(screen.getAllByRole("combobox")).toHaveLength(7);
    expect(screen.getByRole("checkbox", { name: "Reduce motion" })).toBeInTheDocument();
    expect(screen.getByText("workspace settings")).toBeInTheDocument();
    expect(screen.getByText("The current workspace supplies this value.")).toBeInTheDocument();
    expect(screen.getByText("An environment variable currently has precedence.")).toBeInTheDocument();
    expect(screen.getByText("Your saved value is currently overridden.")).toBeInTheDocument();
    expect(screen.getByRole("navigation", { name: "Settings sections" })).toBeInTheDocument();
  });

  it("issues a typed host command for every setting and a null reset", async () => {
    const { commands, user } = renderWorkspace();

    await user.selectOptions(screen.getByRole("combobox", { name: "Theme identity" }), "rose");
    await user.selectOptions(screen.getByRole("combobox", { name: "Color and contrast" }), "no-color");
    await user.selectOptions(screen.getByRole("combobox", { name: "Interface density" }), "compact");
    await user.selectOptions(screen.getByRole("combobox", { name: "Interface zoom" }), "150");
    await user.selectOptions(screen.getByRole("combobox", { name: "Conversation font size" }), "large");
    await user.click(screen.getByRole("checkbox", { name: "Reduce motion" }));
    await user.selectOptions(screen.getByRole("combobox", { name: "Timestamps" }), "absolute");
    await user.selectOptions(screen.getByRole("combobox", { name: "Submit prompts with" }), "control_s");
    await user.click(screen.getByRole("button", { name: "Reset Submit prompts with to its inherited value" }));

    await waitFor(() => expect(commands).toHaveLength(9));
    expect(commands).toEqual([
      { type: "update_client_preference", change: { kind: "theme_preset", value: "rose" } },
      { type: "update_client_preference", change: { kind: "color_mode", value: "no-color" } },
      { type: "update_client_preference", change: { kind: "density", value: "compact" } },
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
    expect(screen.queryByRole("heading", { name: "Appearance" })).not.toBeInTheDocument();
    expect(screen.getByRole("navigation", { name: "Settings sections" })).toBeInTheDocument();

    await user.clear(screen.getByRole("searchbox", { name: "Search settings" }));
    await user.type(screen.getByRole("searchbox", { name: "Search settings" }), "unrelated");
    expect(screen.getByRole("heading", { name: /No settings match/ })).toBeInTheDocument();
  });
});

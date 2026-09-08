import { useState, type ReactNode } from "react";
import type {
  ClientCommand,
  ClientPreferenceChange,
  ClientSettingsProjection,
  ColorMode,
  CommandOutcome,
  ComposerSubmitBehavior,
  EffectiveSetting,
  GuiFontSize,
  PreferenceSource,
  TimestampStyle,
} from "../protocol";
import { COLOR_MODES, THEME_PRESETS } from "../design-system/appearance";
import { Icon } from "./Icon";
import { Button } from "./primitives";
import { ThemePicker } from "./ThemePicker";

interface SettingsWorkspaceProps {
  settings: ClientSettingsProjection;
  onCommand: (command: ClientCommand) => Promise<CommandOutcome>;
  onOpenNavigation: () => void;
}

const SOURCE_LABELS: Record<PreferenceSource, string> = {
  default: "built-in default",
  user_file: "your settings",
  workspace_file: "workspace settings",
  environment: "environment",
  command_line: "launch option",
};

const SOURCE_EXPLANATIONS: Record<PreferenceSource, string> = {
  default: "Default value.",
  user_file: "This value is stored in your local AutoHarness profile.",
  workspace_file: "The current workspace supplies this value.",
  environment: "An environment variable currently has precedence.",
  command_line: "A launch option currently has highest precedence.",
};

const ZOOM_LEVELS = [75, 90, 100, 110, 125, 150, 175, 200] as const;
const FONT_SIZES: readonly [GuiFontSize, string][] = [
  ["small", "Small"],
  ["standard", "Standard"],
  ["large", "Large"],
  ["extra_large", "Extra large"],
];
const TIMESTAMPS: readonly [TimestampStyle, string][] = [
  ["relative", "Relative"],
  ["absolute", "Absolute"],
  ["hidden", "Hidden"],
];
const SUBMISSION_BEHAVIORS: readonly [ComposerSubmitBehavior, string][] = [
  ["control_s", "Ctrl/Cmd + S"],
  ["enter", "Enter"],
];

interface SettingShellProps<T> {
  children: ReactNode;
  visible: boolean;
  description: string;
  id: string;
  label: string;
  setting: EffectiveSetting<T>;
  busy: boolean;
  onReset: () => void;
}

function SettingShell<T>({ children, description, id, label, setting, busy, onReset, visible }: SettingShellProps<T>) {
  if (!visible) return null;
  const sourceLabel = SOURCE_LABELS[setting.source];
  return (
    <div className="settingRow" data-setting={id}>
      <div className="settingCopy">
        <label htmlFor={id} id={`${id}-label`}>{label}</label>
        <p id={`${id}-description`}>{description}</p>
        {setting.source !== "default" && setting.source !== "user_file" ? <p className="settingProvenance" id={`${id}-source`}>
          <span title={SOURCE_EXPLANATIONS[setting.source]}>{sourceLabel}</span>
          {setting.userOverride ? <strong>Your saved value is currently overridden.</strong> : null}
        </p> : <span className="srOnly" id={`${id}-source`}>{sourceLabel}</span>}
      </div>
      <div className="settingControl">
        {children}
        {setting.userOverride ? <Button
          aria-label={`Reset ${label} to its inherited value`}
          disabled={!setting.userOverride}
          loading={busy}
          loadingLabel="Resetting"
          onClick={onReset}
          size="small"
          title={setting.userOverride ? "Remove your saved override" : "No saved user override to remove"}
          variant="quiet"
        >Reset</Button> : null}
      </div>
    </div>
  );
}

export function SettingsWorkspace({ settings, onCommand, onOpenNavigation }: SettingsWorkspaceProps) {
  const [query, setQuery] = useState("");
  const [busy, setBusy] = useState<string>();
  const [message, setMessage] = useState("");
  const [section, setSection] = useState("appearance");
  const rows = [
    { id: "theme-preset", section: "appearance", terms: "theme identity palette system " + THEME_PRESETS.flat().join(" ") },
    { id: "color-mode", section: "appearance", terms: "color contrast saturation " + COLOR_MODES.flat().join(" ") },
    { id: "zoom-percent", section: "accessibility", terms: "interface zoom scale 75 90 100 110 125 150 175 200" },
    { id: "font-size", section: "accessibility", terms: "conversation font text size small standard large extra large" },
    { id: "reduced-motion", section: "accessibility", terms: "reduce motion animation transitions on off" },
    { id: "timestamp-style", section: "conversation", terms: "timestamps time dates relative absolute hidden" },
    { id: "submission-behavior", section: "conversation", terms: "submit prompts send keyboard enter control command ctrl cmd multiline" },
  ];
  const needle = query.trim().toLocaleLowerCase();
  const visibleRows = rows.filter((row) => needle ? `${row.section} ${row.terms}`.toLocaleLowerCase().includes(needle) : row.section === section);
  const visibleSections = new Set(visibleRows.map((row) => row.section));
  const visible = (id: string) => visibleRows.some((row) => row.id === id);

  const update = async (change: ClientPreferenceChange, label: string) => {
    if (busy) return;
    setBusy(change.kind);
    setMessage("");
    const outcome = await onCommand({ type: "update_client_preference", change });
    setBusy(undefined);
    setMessage(outcome === "committed"
      ? `${label} updated.`
      : outcome === "unknown"
        ? `${label} may have changed. AutoHarness is reconciling with the host.`
        : `${label} was not changed.`);
  };

  const describedBy = (id: string) => `${id}-description ${id}-source`;
  const reset = (kind: ClientPreferenceChange["kind"], label: string) => {
    void update({ kind, value: null } as ClientPreferenceChange, label);
  };

  return (
    <main className="routeWorkspace settingsRouteWorkspace" id="main-content" tabIndex={-1}>
      <header className="routeWorkspaceHeader settingsWorkspaceHeader">
        <button aria-label="Open navigation" className="iconButton mobileMenu" onClick={onOpenNavigation} type="button"><Icon name="menu" /></button>
        <div><h1>Settings</h1></div>
        <label className="routeSearch"><Icon name="search" size={16} /><span className="srOnly">Search settings</span><input onChange={(event) => setQuery(event.target.value.slice(0, 64))} placeholder="Search settings" type="search" value={query} /></label>
      </header>

      <div className="settingsLayout">
        <nav aria-label="Settings sections" className="settingsNav">
          {(["appearance", "accessibility", "conversation"] as const).map((value) => <button aria-pressed={!needle && section === value} key={value} onClick={() => { setSection(value); setQuery(""); }} type="button">{value[0]!.toUpperCase() + value.slice(1)}</button>)}
        </nav>

        <div className="settingsSections">
          {visibleSections.has("appearance") ? (
            <section aria-labelledby="settings-appearance-heading" className="settingsWorkspace" id="settings-appearance">
              <header className={needle ? undefined : "srOnly"}><h2 id="settings-appearance-heading">Appearance</h2></header>
              <SettingShell visible={visible("theme-preset")} busy={busy === "theme_preset"} description="Choose a theme, or follow your system." id="theme-preset" label="Theme" onReset={() => reset("theme_preset", "Theme")} setting={settings.themePreset}>
                <ThemePicker colorMode={settings.colorMode.value} describedBy={describedBy("theme-preset")} disabled={Boolean(busy)} onChange={(value) => void update({ kind: "theme_preset", value }, "Theme")} value={settings.themePreset.value} />
              </SettingShell>
              <SettingShell visible={visible("color-mode")} busy={busy === "color_mode"} description="Adjust color saturation and contrast." id="color-mode" label="Color and contrast" onReset={() => reset("color_mode", "Color and contrast")} setting={settings.colorMode}>
                <select aria-describedby={describedBy("color-mode")} disabled={Boolean(busy)} id="color-mode" onChange={(event) => void update({ kind: "color_mode", value: event.target.value as ColorMode }, "Color and contrast")} value={settings.colorMode.value}>{COLOR_MODES.map(([value, label]) => <option key={value} value={value}>{label}</option>)}</select>
              </SettingShell>
            </section>
          ) : null}

          {visibleSections.has("accessibility") ? (
            <section aria-labelledby="settings-accessibility-heading" className="settingsWorkspace" id="settings-accessibility">
              <header className={needle ? undefined : "srOnly"}><h2 id="settings-accessibility-heading">Accessibility</h2></header>
              <SettingShell visible={visible("zoom-percent")} busy={busy === "zoom_percent"} description="Make everything on screen larger or smaller." id="zoom-percent" label="Zoom" onReset={() => reset("zoom_percent", "Zoom")} setting={settings.zoomPercent}>
                <select aria-describedby={describedBy("zoom-percent")} disabled={Boolean(busy)} id="zoom-percent" onChange={(event) => void update({ kind: "zoom_percent", value: Number(event.target.value) }, "Zoom")} value={settings.zoomPercent.value}>{ZOOM_LEVELS.map((value) => <option key={value} value={value}>{value}%</option>)}</select>
              </SettingShell>
              <SettingShell visible={visible("font-size")} busy={busy === "font_size"} description="Change the text size in conversations." id="font-size" label="Text size" onReset={() => reset("font_size", "Text size")} setting={settings.fontSize}>
                <select aria-describedby={describedBy("font-size")} disabled={Boolean(busy)} id="font-size" onChange={(event) => void update({ kind: "font_size", value: event.target.value as GuiFontSize }, "Text size")} value={settings.fontSize.value}>{FONT_SIZES.map(([value, label]) => <option key={value} value={value}>{label}</option>)}</select>
              </SettingShell>
              <SettingShell visible={visible("reduced-motion")} busy={busy === "reduced_motion"} description="Limit animation. Also follows your system preference." id="reduced-motion" label="Reduce motion" onReset={() => reset("reduced_motion", "Reduce motion")} setting={settings.reducedMotion}>
                <label className="switchControl" htmlFor="reduced-motion"><input aria-describedby={describedBy("reduced-motion")} aria-label="Reduce motion" checked={settings.reducedMotion.value} disabled={Boolean(busy)} id="reduced-motion" onChange={(event) => void update({ kind: "reduced_motion", value: event.target.checked }, "Reduce motion")} type="checkbox" /><span aria-hidden="true" /><span>{settings.reducedMotion.value ? "On" : "Off"}</span></label>
              </SettingShell>
            </section>
          ) : null}

          {visibleSections.has("conversation") ? (
            <section aria-labelledby="settings-conversation-heading" className="settingsWorkspace" id="settings-conversation">
              <header className={needle ? undefined : "srOnly"}><h2 id="settings-conversation-heading">Conversation</h2></header>
              <SettingShell visible={visible("timestamp-style")} busy={busy === "timestamp_style"} description="Show relative times, exact local dates and times, or no timestamps." id="timestamp-style" label="Timestamps" onReset={() => reset("timestamp_style", "Timestamps")} setting={settings.timestampStyle}>
                <select aria-describedby={describedBy("timestamp-style")} disabled={Boolean(busy)} id="timestamp-style" onChange={(event) => void update({ kind: "timestamp_style", value: event.target.value as TimestampStyle }, "Timestamps")} value={settings.timestampStyle.value}>{TIMESTAMPS.map(([value, label]) => <option key={value} value={value}>{label}</option>)}</select>
              </SettingShell>
              <SettingShell visible={visible("submission-behavior")} busy={busy === "composer_submit_behavior"} description="Choose a keyboard shortcut to send messages." id="submission-behavior" label="Send with" onReset={() => reset("composer_submit_behavior", "Submission behavior")} setting={settings.composerSubmitBehavior}>
                <select aria-describedby={describedBy("submission-behavior")} disabled={Boolean(busy)} id="submission-behavior" onChange={(event) => void update({ kind: "composer_submit_behavior", value: event.target.value as ComposerSubmitBehavior }, "Submission behavior")} value={settings.composerSubmitBehavior.value}>{SUBMISSION_BEHAVIORS.map(([value, label]) => <option key={value} value={value}>{label}</option>)}</select>
              </SettingShell>
            </section>
          ) : null}

          {visibleSections.size === 0 ? <div className="settingsEmpty"><Icon name="search" size={23} /><h2>No settings match “{query}”</h2><p>Try theme, zoom, motion, timestamps, or submission.</p><Button variant="quiet" onClick={() => setQuery("")}>Clear search</Button></div> : null}
        </div>
      </div>
      <p aria-atomic="true" aria-live="polite" className="settingsAnnouncer">{message}</p>
    </main>
  );
}

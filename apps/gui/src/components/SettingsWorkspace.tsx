import { useMemo, useState, type ReactNode } from "react";
import type {
  ClientCommand,
  ClientPreferenceChange,
  ClientSettingsProjection,
  ColorMode,
  CommandOutcome,
  ComposerSubmitBehavior,
  Density,
  EffectiveSetting,
  GuiFontSize,
  PreferenceSource,
  ThemePreset,
  TimestampStyle,
} from "../protocol";
import { COLOR_MODES, THEME_PRESETS } from "../design-system/appearance";
import { Icon } from "./Icon";
import { Button, Chip } from "./primitives";

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
  default: "No configured layer overrides the built-in fallback.",
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
const DENSITIES: readonly [Density, string][] = [
  ["comfortable", "Comfortable"],
  ["compact", "Compact"],
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
  description: string;
  id: string;
  label: string;
  setting: EffectiveSetting<T>;
  busy: boolean;
  onReset: () => void;
}

function SettingShell<T>({ children, description, id, label, setting, busy, onReset }: SettingShellProps<T>) {
  const sourceLabel = SOURCE_LABELS[setting.source];
  return (
    <div className="settingRow" data-setting={id}>
      <div className="settingCopy">
        <label htmlFor={id}>{label}</label>
        <p id={`${id}-description`}>{description}</p>
        <p className="settingProvenance" id={`${id}-source`}>
          <Chip intent={setting.source === "user_file" ? "info" : "neutral"}>{sourceLabel}</Chip>
          <span>{SOURCE_EXPLANATIONS[setting.source]}</span>
          {setting.userOverride && setting.source !== "user_file" ? <strong>Your saved value is currently overridden.</strong> : null}
        </p>
      </div>
      <div className="settingControl">
        {children}
        <Button
          aria-label={`Reset ${label} to its inherited value`}
          disabled={!setting.userOverride}
          loading={busy}
          loadingLabel="Resetting"
          onClick={onReset}
          size="small"
          title={setting.userOverride ? "Remove your saved override" : "No saved user override to remove"}
          variant="quiet"
        >Reset</Button>
      </div>
    </div>
  );
}

export function SettingsWorkspace({ settings, onCommand, onOpenNavigation }: SettingsWorkspaceProps) {
  const [query, setQuery] = useState("");
  const [busy, setBusy] = useState<string>();
  const [message, setMessage] = useState("");
  const visibleSections = useMemo(() => {
    const needle = query.trim().toLocaleLowerCase();
    if (!needle) return new Set(["appearance", "accessibility", "conversation"]);
    const groups = {
      appearance: "appearance theme color contrast density",
      accessibility: "accessibility zoom font size motion animation reduced",
      conversation: "conversation timestamp time submission keyboard enter control command",
    };
    return new Set(Object.entries(groups).filter(([, terms]) => terms.includes(needle)).map(([key]) => key));
  }, [query]);

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
        <div><p className="eyebrow">Personalization</p><h1>Settings</h1><p>Inspect every desktop preference, see why it won, and reset your local override.</p></div>
        <label className="routeSearch"><Icon name="search" size={16} /><span className="srOnly">Search settings</span><input onChange={(event) => setQuery(event.target.value.slice(0, 64))} placeholder="Search settings" type="search" value={query} /></label>
      </header>

      <div className="settingsLayout">
        <nav aria-label="Settings sections" className="settingsNav">
          <a href="#settings-appearance">Appearance</a>
          <a href="#settings-accessibility">Accessibility</a>
          <a href="#settings-conversation">Conversation</a>
          <div className="settingsAuthority"><Icon name="shield" size={15} /><span><strong>Rust-owned settings</strong><small>Changes persist through the local profile boundary.</small></span></div>
        </nav>

        <div className="settingsSections">
          {visibleSections.has("appearance") ? (
            <section aria-labelledby="settings-appearance-heading" className="settingsWorkspace" id="settings-appearance">
              <header><p className="eyebrow">Interface</p><h2 id="settings-appearance-heading">Appearance</h2><p>Theme, color treatment, and spacing apply across supported renderers.</p></header>
              <SettingShell busy={busy === "theme_preset"} description="Choose a renderer-neutral palette, or follow the operating system." id="theme-preset" label="Theme identity" onReset={() => reset("theme_preset", "Theme identity")} setting={settings.themePreset}>
                <select aria-describedby={describedBy("theme-preset")} disabled={Boolean(busy)} id="theme-preset" onChange={(event) => void update({ kind: "theme_preset", value: event.target.value as ThemePreset }, "Theme identity")} value={settings.themePreset.value}>{THEME_PRESETS.map(([value, label]) => <option key={value} value={value}>{label}</option>)}</select>
              </SettingShell>
              <SettingShell busy={busy === "color_mode"} description="Adjust saturation or use a no-color or high-contrast treatment." id="color-mode" label="Color and contrast" onReset={() => reset("color_mode", "Color and contrast")} setting={settings.colorMode}>
                <select aria-describedby={describedBy("color-mode")} disabled={Boolean(busy)} id="color-mode" onChange={(event) => void update({ kind: "color_mode", value: event.target.value as ColorMode }, "Color and contrast")} value={settings.colorMode.value}>{COLOR_MODES.map(([value, label]) => <option key={value} value={value}>{label}</option>)}</select>
              </SettingShell>
              <SettingShell busy={busy === "density"} description="Use comfortable spacing or fit more information on screen." id="density" label="Interface density" onReset={() => reset("density", "Interface density")} setting={settings.density}>
                <select aria-describedby={describedBy("density")} disabled={Boolean(busy)} id="density" onChange={(event) => void update({ kind: "density", value: event.target.value as Density }, "Interface density")} value={settings.density.value}>{DENSITIES.map(([value, label]) => <option key={value} value={value}>{label}</option>)}</select>
              </SettingShell>
              <div className="themePreview" aria-label={`${settings.themePreset.value} theme with ${settings.colorMode.value} color treatment`}><span /><span /><span /><strong>{THEME_PRESETS.find(([value]) => value === settings.themePreset.value)?.[1]}</strong><small>{COLOR_MODES.find(([value]) => value === settings.colorMode.value)?.[1]} treatment</small></div>
            </section>
          ) : null}

          {visibleSections.has("accessibility") ? (
            <section aria-labelledby="settings-accessibility-heading" className="settingsWorkspace" id="settings-accessibility">
              <header><p className="eyebrow">Reading and motion</p><h2 id="settings-accessibility-heading">Accessibility</h2><p>Scale the desktop canvas and make movement predictable.</p></header>
              <SettingShell busy={busy === "zoom_percent"} description="Scale the complete interface from 75 to 200 percent without hiding primary actions." id="zoom-percent" label="Interface zoom" onReset={() => reset("zoom_percent", "Interface zoom")} setting={settings.zoomPercent}>
                <select aria-describedby={describedBy("zoom-percent")} disabled={Boolean(busy)} id="zoom-percent" onChange={(event) => void update({ kind: "zoom_percent", value: Number(event.target.value) }, "Interface zoom")} value={settings.zoomPercent.value}>{ZOOM_LEVELS.map((value) => <option key={value} value={value}>{value}%</option>)}</select>
              </SettingShell>
              <SettingShell busy={busy === "font_size"} description="Adjust conversation and composer text independently of the full interface zoom." id="font-size" label="Conversation font size" onReset={() => reset("font_size", "Conversation font size")} setting={settings.fontSize}>
                <select aria-describedby={describedBy("font-size")} disabled={Boolean(busy)} id="font-size" onChange={(event) => void update({ kind: "font_size", value: event.target.value as GuiFontSize }, "Conversation font size")} value={settings.fontSize.value}>{FONT_SIZES.map(([value, label]) => <option key={value} value={value}>{label}</option>)}</select>
              </SettingShell>
              <SettingShell busy={busy === "reduced_motion"} description="Remove spatial transitions and looping animation. System reduced-motion is also respected." id="reduced-motion" label="Reduce motion" onReset={() => reset("reduced_motion", "Reduce motion")} setting={settings.reducedMotion}>
                <label className="switchControl" htmlFor="reduced-motion"><input aria-describedby={describedBy("reduced-motion")} aria-label="Reduce motion" checked={settings.reducedMotion.value} disabled={Boolean(busy)} id="reduced-motion" onChange={(event) => void update({ kind: "reduced_motion", value: event.target.checked }, "Reduce motion")} type="checkbox" /><span aria-hidden="true" /><span>{settings.reducedMotion.value ? "On" : "Off"}</span></label>
              </SettingShell>
            </section>
          ) : null}

          {visibleSections.has("conversation") ? (
            <section aria-labelledby="settings-conversation-heading" className="settingsWorkspace" id="settings-conversation">
              <header><p className="eyebrow">Conversation</p><h2 id="settings-conversation-heading">Time and submission</h2><p>Control how replay history is dated and how multiline prompts are sent.</p></header>
              <SettingShell busy={busy === "timestamp_style"} description="Show relative times, exact local dates and times, or no timestamps." id="timestamp-style" label="Timestamps" onReset={() => reset("timestamp_style", "Timestamps")} setting={settings.timestampStyle}>
                <select aria-describedby={describedBy("timestamp-style")} disabled={Boolean(busy)} id="timestamp-style" onChange={(event) => void update({ kind: "timestamp_style", value: event.target.value as TimestampStyle }, "Timestamps")} value={settings.timestampStyle.value}>{TIMESTAMPS.map(([value, label]) => <option key={value} value={value}>{label}</option>)}</select>
              </SettingShell>
              <SettingShell busy={busy === "composer_submit_behavior"} description="Choose Enter for rapid chat or Ctrl/Cmd + S for safer multiline composition." id="submission-behavior" label="Submit prompts with" onReset={() => reset("composer_submit_behavior", "Submission behavior")} setting={settings.composerSubmitBehavior}>
                <select aria-describedby={describedBy("submission-behavior")} disabled={Boolean(busy)} id="submission-behavior" onChange={(event) => void update({ kind: "composer_submit_behavior", value: event.target.value as ComposerSubmitBehavior }, "Submission behavior")} value={settings.composerSubmitBehavior.value}>{SUBMISSION_BEHAVIORS.map(([value, label]) => <option key={value} value={value}>{label}</option>)}</select>
              </SettingShell>
            </section>
          ) : null}

          {visibleSections.size === 0 ? <div className="settingsEmpty"><Icon name="search" size={23} /><h2>No settings match “{query}”</h2><p>Try theme, zoom, motion, timestamps, or submission.</p></div> : null}
        </div>
      </div>
      <p aria-atomic="true" aria-live="polite" className="settingsAnnouncer">{message}</p>
    </main>
  );
}

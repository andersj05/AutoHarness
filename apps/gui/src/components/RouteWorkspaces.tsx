import { COLOR_MODES, THEME_PRESETS, type ColorMode, type ThemePreset } from "../design-system/appearance";
import { Icon } from "./Icon";

interface SimpleWorkspaceProps {
  route: "memory" | "settings";
  colorMode: ColorMode;
  reduceMotion: boolean;
  theme: ThemePreset;
  onColorMode: (value: ColorMode) => void;
  onOpenNavigation: () => void;
  onReduceMotion: (value: boolean) => void;
  onTheme: (value: ThemePreset) => void;
}

export function SimpleWorkspace({ route, colorMode, reduceMotion, theme, onColorMode, onOpenNavigation, onReduceMotion, onTheme }: SimpleWorkspaceProps) {
  const memory = route === "memory";
  return (
    <main className="routeWorkspace" id="main-content">
      <header className="routeWorkspaceHeader simple">
        <button aria-label="Open navigation" className="iconButton mobileMenu" onClick={onOpenNavigation} type="button"><Icon name="menu" /></button>
        <div><p className="eyebrow">{memory ? "Knowledge ledger" : "Personalization"}</p><h1>{memory ? "Memory" : "Settings"}</h1><p>{memory ? "Provenance-rich memory remains under durable host authority." : "Tune the presentation without changing runtime policy."}</p></div>
      </header>
      {memory ? (
        <section className="futureWorkspace">
          <span className="futureIcon"><Icon name="memory" size={27} /></span>
          <div><p className="eyebrow">Migration stage 7</p><h2>Memory deserves a richer canvas</h2><p>The desktop surface will add provenance timelines, relation views, admission history, and safe diffs while preserving review-only proposals.</p></div>
          <div className="futurePreview" aria-hidden="true"><i /><i /><i /><i /></div>
        </section>
      ) : (
        <section className="settingsWorkspace" aria-labelledby="appearance-heading">
          <div><p className="eyebrow">Interface</p><h2 id="appearance-heading">Appearance and motion</h2><p>System preferences remain the default. These preview controls are presentation-only.</p></div>
          <label className="settingRow"><span><strong>Theme identity</strong><small>Preview all nine renderer-neutral appearance seeds.</small></span><select aria-label="Theme identity" onChange={(event) => onTheme(event.target.value as ThemePreset)} value={theme}>{THEME_PRESETS.map(([value, label]) => <option key={value} value={value}>{label}</option>)}</select></label>
          <label className="settingRow"><span><strong>Color treatment</strong><small>Preserve state through labels, icons, outlines, and patterns.</small></span><select aria-label="Color treatment" onChange={(event) => onColorMode(event.target.value as ColorMode)} value={colorMode}>{COLOR_MODES.map(([value, label]) => <option key={value} value={value}>{label}</option>)}</select></label>
          <label className="settingRow"><span><strong>Reduce motion</strong><small>Freeze looping activity and remove spatial transitions.</small></span><input checked={reduceMotion} onChange={(event) => onReduceMotion(event.target.checked)} type="checkbox" /></label>
          <div className="themePreview"><span /><span /><span /><strong>{THEME_PRESETS.find(([value]) => value === theme)?.[1]}</strong><small>{COLOR_MODES.find(([value]) => value === colorMode)?.[1]} treatment</small></div>
        </section>
      )}
    </main>
  );
}

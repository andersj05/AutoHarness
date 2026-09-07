import { useId } from "react";
import { THEME_PRESETS } from "../design-system/appearance";
import type { ColorMode, ThemePreset } from "../protocol";
import { Icon } from "./Icon";
import "./ThemePicker.css";

export function ThemePicker({ value, colorMode, disabled, onChange, describedBy }: {
  value: ThemePreset;
  colorMode: ColorMode;
  disabled: boolean;
  onChange: (theme: ThemePreset) => void;
  describedBy: string;
}) {
  const name = useId();
  return <div aria-describedby={describedBy} aria-labelledby="theme-preset-label" className="themePicker" id="theme-preset" role="radiogroup">
    {THEME_PRESETS.map(([theme, label], index) => <label className="themeChoice" data-featured={index < 3} key={theme}>
      <input checked={value === theme} disabled={disabled} name={name} onChange={() => onChange(theme)} type="radio" value={theme} />
      <span aria-hidden="true" className="themeThumbnail" data-color-mode={colorMode} data-theme={theme === "system" ? "light" : theme}>
        <span className="themeMiniRail"><i /><i /><i /></span>
        <span className="themeMiniPage"><i /><i /><i /><b /></span>
        {theme === "system" ? <span className="themeSystemHalf" data-color-mode={colorMode} data-theme="dark"><i /><i /><b /></span> : null}
      </span>
      <span className="themeChoiceLabel"><span>{label}</span><Icon name="check" size={14} /></span>
    </label>)}
  </div>;
}

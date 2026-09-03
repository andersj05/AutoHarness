export const THEME_PRESETS = [
  ["system", "System"],
  ["light", "Light"],
  ["dark", "Dark"],
  ["aurora", "Aurora"],
  ["ember", "Ember"],
  ["midnight", "Midnight"],
  ["ocean", "Ocean"],
  ["forest", "Forest"],
  ["rose", "Rose"],
] as const;

export const COLOR_MODES = [
  ["color", "Color"],
  ["soft", "Soft"],
  ["vivid", "Vivid"],
  ["no-color", "No color"],
  ["high-contrast", "High contrast"],
] as const;

export type ThemePreset = typeof THEME_PRESETS[number][0];
export type ColorMode = typeof COLOR_MODES[number][0];

import { useState, type CSSProperties, type PropsWithChildren, type ReactNode } from "react";

export interface SplitPaneProps extends PropsWithChildren {
  label: string;
  maxPercent?: number;
  minPercent?: number;
  onValueChange?: (percent: number) => void;
  secondary: ReactNode;
  value?: number;
}

export function SplitPane({ children, label, maxPercent = 80, minPercent = 20, onValueChange, secondary, value }: SplitPaneProps) {
  const [internalValue, setInternalValue] = useState(65);
  const current = value ?? internalValue;
  const update = (next: number) => {
    const clamped = Math.min(maxPercent, Math.max(minPercent, Math.round(next)));
    if (value === undefined) setInternalValue(clamped);
    onValueChange?.(clamped);
  };
  return (
    <div className="dsSplitPane" style={{ "--split-primary": `${current}%` } as CSSProperties}>
      <div className="dsSplitPrimary">{children}</div>
      <button
        aria-label={label}
        aria-orientation="vertical"
        aria-valuemax={maxPercent}
        aria-valuemin={minPercent}
        aria-valuenow={current}
        className="dsSplitHandle"
        onKeyDown={(event) => {
          if (event.key === "ArrowLeft") {
            event.preventDefault();
            update(current - (event.shiftKey ? 10 : 2));
          } else if (event.key === "ArrowRight") {
            event.preventDefault();
            update(current + (event.shiftKey ? 10 : 2));
          } else if (event.key === "Home") {
            event.preventDefault();
            update(minPercent);
          } else if (event.key === "End") {
            event.preventDefault();
            update(maxPercent);
          }
        }}
        onPointerMove={(event) => {
          if (event.buttons !== 1) return;
          const bounds = event.currentTarget.parentElement?.getBoundingClientRect();
          if (bounds?.width) update((event.clientX - bounds.left) / bounds.width * 100);
        }}
        role="separator"
        type="button"
      ><span /></button>
      <div className="dsSplitSecondary">{secondary}</div>
    </div>
  );
}

import type { HTMLAttributes, ReactNode } from "react";
import { Icon, type IconName } from "../Icon";

export interface ChipProps extends HTMLAttributes<HTMLSpanElement> {
  icon?: IconName;
  intent?: "neutral" | "info" | "success" | "warning" | "danger";
  children: ReactNode;
}

export function Chip({ children, className = "", icon, intent = "neutral", ...props }: ChipProps) {
  return (
    <span {...props} className={`dsChip ${className}`.trim()} data-intent={intent}>
      {icon ? <Icon name={icon} size={12} /> : <span aria-hidden="true" className="dsChipMark" />}
      <span>{children}</span>
    </span>
  );
}

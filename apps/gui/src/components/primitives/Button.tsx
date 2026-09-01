import type { ButtonHTMLAttributes, ReactNode } from "react";
import { Icon, type IconName } from "../Icon";

export interface ButtonProps extends ButtonHTMLAttributes<HTMLButtonElement> {
  icon?: IconName;
  loading?: boolean;
  loadingLabel?: string;
  size?: "small" | "medium";
  variant?: "primary" | "secondary" | "quiet" | "danger";
  children: ReactNode;
}

export function Button({
  children,
  className = "",
  disabled,
  icon,
  loading = false,
  loadingLabel = "Working",
  size = "medium",
  variant = "secondary",
  ...props
}: ButtonProps) {
  return (
    <button
      {...props}
      aria-busy={loading || undefined}
      className={`dsButton ${className}`.trim()}
      data-size={size}
      data-variant={variant}
      disabled={disabled || loading}
      type={props.type ?? "button"}
    >
      {loading ? <span aria-hidden="true" className="dsButtonSpinner" /> : icon ? <Icon name={icon} size={15} /> : null}
      <span>{loading ? loadingLabel : children}</span>
    </button>
  );
}

import type { ReactNode } from "react";
import { Icon, type IconName } from "../Icon";

export interface StatusSurfaceProps {
  action?: ReactNode;
  detail: string;
  icon?: IconName;
  intent?: "info" | "success" | "warning" | "danger";
  title: string;
}

export function StatusSurface({ action, detail, icon = "spark", intent = "info", title }: StatusSurfaceProps) {
  return (
    <section aria-label={`${intent}: ${title}`} className="dsStatusSurface" data-intent={intent} role={intent === "danger" ? "alert" : "status"}>
      <span className="dsStatusIcon"><Icon name={icon} size={22} /></span>
      <div><strong>{title}</strong><p>{detail}</p></div>
      {action ? <div className="dsStatusAction">{action}</div> : null}
    </section>
  );
}

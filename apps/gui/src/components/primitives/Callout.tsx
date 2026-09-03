import type { ReactNode } from "react";
import { Icon, type IconName } from "../Icon";

export interface CalloutProps {
  action?: ReactNode;
  detail: string;
  icon?: IconName;
  intent?: "info" | "success" | "warning" | "danger";
  title: string;
}

export function Callout({ action, detail, icon = "warning", intent = "info", title }: CalloutProps) {
  return (
    <section aria-label={`${intent}: ${title}`} className="dsCallout callout" data-intent={intent} role={intent === "danger" ? "alert" : "status"}>
      <span className="calloutIcon"><Icon name={icon} /></span>
      <div><strong>{title}</strong><p>{detail}</p></div>
      {action ? <div className="dsCalloutAction">{action}</div> : null}
    </section>
  );
}

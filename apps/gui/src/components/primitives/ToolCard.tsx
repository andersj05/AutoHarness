import type { ReactNode } from "react";
import { Icon } from "../Icon";
import { Chip } from "./Chip";

export type ToolCardStatus = "queued" | "waiting" | "running" | "denying" | "succeeded" | "denied" | "failed" | "cancelled";

export interface ToolCardProps {
  children?: ReactNode;
  name: string;
  resource: string;
  status: ToolCardStatus;
  summary: string;
}

function statusIntent(status: ToolCardStatus): "neutral" | "info" | "success" | "warning" | "danger" {
  if (status === "succeeded") return "success";
  if (status === "failed" || status === "denied") return "danger";
  if (status === "running") return "info";
  if (status === "cancelled" || status === "denying") return "warning";
  return "neutral";
}

export function ToolCard({ children, name, resource, status, summary }: ToolCardProps) {
  return (
    <details className="dsToolCard toolCard" data-status={status}>
      <summary>
        <span className="toolIcon"><Icon name="terminal" size={15} /></span>
        <span className="toolSummary"><strong>{name}</strong><span>{summary}</span></span>
        <Chip icon={status === "succeeded" ? "check" : status === "failed" || status === "denied" ? "warning" : undefined} intent={statusIntent(status)}>{status}</Chip>
        <Icon className="disclosureChevron" name="chevron" size={15} />
      </summary>
      <div className="toolDetails">
        <div><span>Resource</span><code>{resource}</code></div>
        {children}
      </div>
    </details>
  );
}

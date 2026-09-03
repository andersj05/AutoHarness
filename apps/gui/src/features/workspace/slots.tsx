import type { ReactNode } from "react";
import { securityDisplaySafe as securityDisplayText } from "../../securityText";

/** Closed presentation slots accept data only, with no host handles or executable actions. */
export type WorkspaceSurface =
  | { kind: "plan"; title: string; steps: readonly { label: string; state: "pending" | "active" | "complete" }[] }
  | { kind: "artifact"; title: string; identity: string; mediaType: string; content: string }
  | { kind: "file"; title: string; path: string; content: string }
  | { kind: "diff"; title: string; before: string; after: string }
  | { kind: "terminal_output"; title: string; command: string; output: string; exitCode: number | null }
  | { kind: "evaluation"; title: string; status: string; metrics: readonly { label: string; value: string }[] };

export interface PresentationSlots {
  inspector?: readonly WorkspaceSurface[];
  route?: ReactNode;
}

export function InertText({ text, className = "" }: { text: string; className?: string }) {
  return <pre className={`inertContent ${className}`}>{text.split("\n").map(securityDisplayText).join("\n")}</pre>;
}

/** A linear two-column comparison preserves every line without parsing patch instructions. */
export function SafeDiff({ before, after }: { before: string; after: string }) {
  return <div className="safeDiff" aria-label="Proposed content comparison">
    <section><h4>Current content</h4><InertText text={before} /></section>
    <section><h4>Proposed content</h4><InertText text={after} /></section>
  </div>;
}

export function WorkspaceSurfaceView({ surface }: { surface: WorkspaceSurface }) {
  let content: ReactNode;
  switch (surface.kind) {
    case "plan": content = <ol className="workspacePlan">{surface.steps.map((step, index) => <li key={index}><span>{step.state}</span> {securityDisplayText(step.label)}</li>)}</ol>; break;
    case "artifact": content = <><p>{securityDisplayText(surface.identity)} · {securityDisplayText(surface.mediaType)}</p><InertText text={surface.content} /></>; break;
    case "file": content = <><p className="monoText">{securityDisplayText(surface.path)}</p><InertText text={surface.content} /></>; break;
    case "diff": content = <SafeDiff before={surface.before} after={surface.after} />; break;
    case "terminal_output": content = <><p>Exit: {surface.exitCode ?? "not reported"}</p><InertText text={surface.command} /><InertText text={surface.output} /></>; break;
    case "evaluation": content = <><p>{securityDisplayText(surface.status)}</p><dl>{surface.metrics.map((metric, index) => <div key={index}><dt>{securityDisplayText(metric.label)}</dt><dd>{securityDisplayText(metric.value)}</dd></div>)}</dl></>; break;
  }
  return <details className="featureSurface"><summary>{securityDisplayText(surface.title)} <small>{surface.kind.replaceAll("_", " ")}</small></summary>{content}</details>;
}

export function InspectorSlot({ surfaces }: { surfaces: readonly WorkspaceSurface[] }) {
  return <section className="inspectorSection" aria-label="Workspace evidence">{surfaces.map((surface, index) => <WorkspaceSurfaceView key={`${surface.kind}-${index}`} surface={surface} />)}</section>;
}

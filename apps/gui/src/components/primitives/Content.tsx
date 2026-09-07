import { securityDisplaySafe as securityDisplayText } from "../../securityText";

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


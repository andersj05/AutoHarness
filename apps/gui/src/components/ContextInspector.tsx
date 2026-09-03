import type { ActivityItem, ActiveSessionProjection, ConnectionState, ModelDescriptor } from "../protocol";
import { Icon } from "./Icon";
import { Chip, Meter } from "./primitives";

interface ContextInspectorProps {
  activity: readonly ActivityItem[];
  connection: ConnectionState;
  mobileOpen: boolean;
  model?: ModelDescriptor;
  runtimeMode: "native" | "fixture";
  session?: ActiveSessionProjection;
  onClose: () => void;
}

function formatTokens(value: bigint): string {
  if (value >= 1_000_000n) return `${value / 1_000_000n}.${value / 100_000n % 10n}m`;
  if (value >= 1_000n) return `${value / 1_000n}.${value / 100n % 10n}k`;
  return value.toString();
}

export function ContextInspector({ activity, connection, mobileOpen, model, runtimeMode, session, onClose }: ContextInspectorProps) {
  const runtimeOnline = connection.kind === "online";
  const latestUsage = session?.attempt.kind === "completed" &&
    session.attempt.inputTokens !== undefined &&
    session.attempt.outputTokens !== undefined
    ? BigInt(session.attempt.inputTokens) + BigInt(session.attempt.outputTokens)
    : undefined;
  const contextWindow = model?.contextWindowTokens ? BigInt(model.contextWindowTokens) : undefined;
  const validUsage = latestUsage !== undefined && latestUsage >= 0n;
  const contextPercent = validUsage && latestUsage !== undefined && contextWindow !== undefined && contextWindow > 0n
    ? Number((latestUsage * 100n / contextWindow) > 100n ? 100n : latestUsage * 100n / contextWindow)
    : 0;
  return (
    <aside aria-label="Context inspector" className="contextInspector" data-mobile-open={mobileOpen}>
      <header className="inspectorHeader">
        <div>
          <p className="eyebrow">{runtimeMode === "fixture" ? "Fixture context" : "Live context"}</p>
          <h2>Inspector</h2>
        </div>
        <button aria-label="Close inspector" className="iconButton inspectorClose" onClick={onClose} type="button">
          <Icon name="close" />
        </button>
      </header>

      <section className="contextMeterSection" aria-labelledby="context-meter-heading">
        <h3 className="srOnly" id="context-meter-heading">Latest turn usage</h3>
        <Meter
          detail={validUsage && latestUsage !== undefined ? `${formatTokens(latestUsage)} reported tokens` : "Usage not reported for this turn"}
          label="Latest turn context usage"
          value={validUsage && contextWindow ? contextPercent : undefined}
        />
        <Chip icon={validUsage ? "check" : "warning"} intent={validUsage ? "success" : "warning"}>
          {validUsage ? "provider reported" : "unavailable"}
        </Chip>
      </section>

      <section className="inspectorSection" aria-labelledby="runtime-heading">
        <div className="sectionTitleRow">
          <h3 id="runtime-heading">Runtime</h3>
          <span className="liveLabel" data-online={runtimeOnline}><span /> {runtimeOnline ? "online" : "local"}</span>
        </div>
        <dl className="inspectorFacts">
          <div><dt>Provider</dt><dd>{connection.kind === "offline" ? "Offline" : connection.providerLabel}</dd></div>
          <div><dt>Model</dt><dd>{model?.displayName ?? "Not selected"}</dd></div>
          <div><dt>Reasoning</dt><dd>{model?.supportsReasoning === true ? "Auto" : model?.supportsReasoning === false ? "Unsupported" : "Unknown"}</dd></div>
          <div><dt>Tools</dt><dd>{model?.supportsTools === true ? "Capability gated" : model?.supportsTools === false ? "Unsupported" : "Unknown"}</dd></div>
        </dl>
      </section>

      <section className="inspectorSection activitySection" aria-labelledby="activity-heading">
        <div className="sectionTitleRow"><h3 id="activity-heading">Turn activity</h3><span>{activity.length} steps</span></div>
        <ol className="activityList">
          {activity.map((item) => (
            <li data-status={item.status} key={item.id}>
              <span className="activityNode">{item.status === "complete" ? <Icon name="check" size={12} /> : null}</span>
              <span><strong>{item.label}</strong><small>{item.detail}</small></span>
            </li>
          ))}
        </ol>
      </section>

      <section className="securityCard">
        <span className="securityCardIcon"><Icon name="shield" /></span>
        {runtimeMode === "fixture" ? (
          <div><strong>Browser fixture</strong><p>Everything shown here is simulated. No Rust host, credential vault, or durable store is connected.</p></div>
        ) : (
          <div><strong>Rust-owned authority</strong><p>Execution and decisions stay in Rust. This pane receives bounded, secret-free projections only.</p></div>
        )}
      </section>
    </aside>
  );
}

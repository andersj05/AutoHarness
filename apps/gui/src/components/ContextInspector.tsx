import { InspectorSlot, type PresentationSlots } from "../features/workspace/slots";
import type { ActivityItem, ActiveSessionProjection, ConnectionState, ModelDescriptor } from "../protocol";
import { Icon } from "./Icon";

interface ContextInspectorProps {
  slots?: Pick<PresentationSlots, "inspector">;
  activity: readonly ActivityItem[];
  connection: ConnectionState;
  mobileOpen: boolean;
  model?: ModelDescriptor;
  runtimeMode: "native" | "fixture";
  session?: ActiveSessionProjection;
  onClose: () => void;
  onChangeModel: () => void;
}

function tokens(value: string | undefined): string {
  return value !== undefined && /^\d+$/.test(value)
    ? new Intl.NumberFormat().format(BigInt(value))
    : "Not reported";
}

const connectionLabels: Record<ConnectionState["kind"], string> = {
  online: "Connected", offline: "Offline", connecting: "Connecting", credential_required: "Sign-in needed",
};

export function ContextInspector({ slots, activity, connection, mobileOpen, model, runtimeMode, session, onClose, onChangeModel }: ContextInspectorProps) {
  const attempt = session?.attempt;
  const usage = attempt?.kind === "completed" ? attempt : undefined;
  const working = attempt?.kind === "streaming" || attempt?.kind === "cancelling";
  return (
    <aside aria-label="Context inspector" className="contextInspector" data-mobile-open={mobileOpen}>
      <header className="inspectorHeader">
        <h2>Session details</h2>
        <button aria-label="Close inspector" className="iconButton inspectorClose" onClick={onClose} type="button"><Icon name="close" /></button>
      </header>

      <section className="inspectorSection inspectorModelSection" aria-labelledby="inspector-model-heading">
        <div className="sectionTitleRow"><h3 id="inspector-model-heading">Model</h3><span className="inspectorConnection" data-online={connection.kind === "online"}>{connectionLabels[connection.kind]}</span></div>
        <button className="inspectorModelButton" disabled={!session || working} onClick={onChangeModel} type="button" aria-label="Change session model">
          <span className="inspectorModelIcon"><Icon name="model" size={20} /></span>
          <span><strong>{model?.displayName ?? "Choose a model"}</strong><small>{model?.provider ?? (connection.kind === "offline" ? "No provider connected" : connection.providerLabel)}</small></span>
          <Icon name="chevron" size={16} />
        </button>
        {model ? <dl className="inspectorCapabilities">
          {model.contextWindowTokens ? <div><dt>Context window</dt><dd>{tokens(model.contextWindowTokens)} tokens</dd></div> : null}
          {model.supportsReasoning !== undefined ? <div><dt>Reasoning</dt><dd>{model.supportsReasoning ? "Supported" : "Not supported"}</dd></div> : null}
          {model.supportsTools !== undefined ? <div><dt>Tools</dt><dd>{model.supportsTools ? "Supported" : "Not supported"}</dd></div> : null}
        </dl> : null}
      </section>

      <section className="inspectorSection" aria-labelledby="turn-usage-heading">
        <div className="sectionTitleRow"><h3 id="turn-usage-heading">Last response</h3></div>
        {usage ? <dl className="inspectorTokenStats">
          <div><dt>Input tokens</dt><dd>{tokens(usage.inputTokens)}</dd></div>
          <div><dt>Output tokens</dt><dd>{tokens(usage.outputTokens)}</dd></div>
        </dl> : <p className="inspectorEmpty">{working ? "Token counts appear when the response finishes." : attempt?.kind === "failed" ? "The response failed. Retry from the conversation." : attempt?.kind === "cancelled" ? "The response was stopped." : "No completed response yet."}</p>}
      </section>

      <section className="inspectorSection activitySection" aria-labelledby="activity-heading">
        <div className="sectionTitleRow"><h3 id="activity-heading">Activity</h3><span>{activity.length ? `${activity.length} ${activity.length === 1 ? "step" : "steps"}` : ""}</span></div>
        {activity.length ? <ol className="activityList">
          {activity.map((item) => (
            <li data-status={item.status} key={item.id}>
              <span className="activityNode" aria-hidden="true">{item.status === "complete" ? <Icon name="check" size={12} /> : item.status === "warning" ? <Icon name="warning" size={11} /> : null}</span>
              <span><strong>{item.label}<span className="srOnly"> ({item.status})</span></strong><small>{item.detail}</small></span>
            </li>
          ))}
        </ol> : <p className="inspectorEmpty">Activity will appear here as you work.</p>}
      </section>

      {slots?.inspector?.length ? <InspectorSlot surfaces={slots.inspector} /> : null}
      {runtimeMode === "fixture" ? <p className="inspectorPreview">Preview data. Changes are not saved.</p> : null}
    </aside>
  );
}

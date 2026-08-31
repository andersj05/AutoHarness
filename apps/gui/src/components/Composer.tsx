import { useEffect, useRef, useState, type Dispatch, type SetStateAction } from "react";
import type { AttemptProjection, CommandOutcome, ModelDescriptor } from "../protocol";
import { Icon } from "./Icon";

interface ComposerProps {
  attempt: AttemptProjection;
  draft: string;
  disabledReason?: string;
  model?: ModelDescriptor;
  runtimeMode: "native" | "fixture";
  onCancel: (attemptId: string) => void;
  onDraftChange: Dispatch<SetStateAction<string>>;
  onOpenModelPicker: () => void;
  onSubmit: (prompt: string) => Promise<CommandOutcome>;
}

export function Composer({ attempt, draft, disabledReason, model, runtimeMode, onCancel, onDraftChange, onOpenModelPicker, onSubmit }: ComposerProps) {
  const [submitting, setSubmitting] = useState(false);
  const textareaRef = useRef<HTMLTextAreaElement>(null);
  const isStreaming = attempt.kind === "streaming" || attempt.kind === "cancelling";
  const canSubmit = draft.trim().length > 0 && !disabledReason && !isStreaming && !submitting && model?.selectable === true;

  useEffect(() => {
    const textarea = textareaRef.current;
    if (!textarea) return;
    textarea.style.height = "0px";
    textarea.style.height = `${Math.min(textarea.scrollHeight, 176)}px`;
  }, [draft]);

  const submit = async () => {
    if (!canSubmit) return;
    const prompt = draft;
    onDraftChange("");
    setSubmitting(true);
    try {
      const outcome = await onSubmit(prompt);
      if (outcome === "rejected") {
        onDraftChange((newerDraft) => newerDraft.length === 0 ? prompt : `${prompt}\n\n${newerDraft}`);
      }
    } finally {
      setSubmitting(false);
    }
  };

  return (
    <div className="composerRegion">
      <div className="composerGlow" />
      <div className="composer" data-streaming={isStreaming}>
        <label className="composerInput">
          <span className="promptGlyph" aria-hidden="true">›</span>
          <span className="srOnly">Message AutoHarness</span>
          <textarea
            aria-describedby={disabledReason ? "composer-disabled-reason" : "composer-help"}
            onChange={(event) => onDraftChange(event.target.value)}
            onKeyDown={(event) => {
              if (event.key === "Enter" && !event.shiftKey && !event.nativeEvent.isComposing) {
                event.preventDefault();
                void submit();
              }
            }}
            placeholder={isStreaming ? "Response in progress" : "Ask AutoHarness anything"}
            ref={textareaRef}
            rows={1}
            spellCheck
            value={draft}
          />
        </label>
        <div className="composerToolbar">
          <div className="composerAccessories">
            <button aria-label={`Change model, current ${model?.displayName ?? "none"}`} className="composerPill" onClick={onOpenModelPicker} type="button">
              <Icon name="model" size={15} />
              <span>{model?.displayName ?? "Choose model"}</span>
              <span className="tinyChevron">⌄</span>
            </button>
            {model?.supportsReasoning === true ? (
              <span className="reasoningPill"><Icon name="spark" size={14} /> reasoning: auto</span>
            ) : null}
          </div>
          {isStreaming && "id" in attempt ? (
            <button
              className="submitButton stopButton"
              disabled={attempt.kind === "cancelling"}
              onClick={() => onCancel(attempt.id)}
              title="Stop response"
              type="button"
            >
              <Icon name="stop" size={16} />
              <span className="srOnly">Stop response</span>
            </button>
          ) : (
            <button className="submitButton" disabled={!canSubmit} onClick={() => void submit()} title="Send message" type="button">
              <Icon name="arrow-up" size={17} />
              <span className="srOnly">Send message</span>
            </button>
          )}
        </div>
      </div>
      <div className="composerFootnote">
        <span id={disabledReason ? "composer-disabled-reason" : "composer-help"}>
          {disabledReason ?? "Enter to send, Shift Enter for a new line"}
        </span>
        <span className="composerSecurity"><Icon name="shield" size={12} /> {runtimeMode === "fixture" ? "simulated, not persisted" : "local and replayable"}</span>
      </div>
    </div>
  );
}

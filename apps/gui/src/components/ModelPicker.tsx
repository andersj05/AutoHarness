import { useMemo, useRef, useState } from "react";
import type { ModelDescriptor } from "../protocol";
import { Dialog } from "./Dialog";
import { Icon } from "./Icon";
import { Button, Chip } from "./primitives";

interface ModelPickerProps {
  models: readonly ModelDescriptor[];
  selectedModelId?: string;
  onClose: () => void;
  onRefresh: () => void;
  onSelect: (modelId: string) => void;
}

function formatContext(tokens?: string): string {
  if (tokens === undefined) return "unknown";
  const value = BigInt(tokens);
  if (value >= 1_000_000n) return `${value / 1_000_000n}.${value / 100_000n % 10n}m`;
  if (value >= 1_000n) return `${value / 1_000n}k`;
  return value.toString();
}

export function ModelPicker({ models, selectedModelId, onClose, onRefresh, onSelect }: ModelPickerProps) {
  const [query, setQuery] = useState("");
  const listRef = useRef<HTMLDivElement>(null);
  const filtered = useMemo(() => {
    const needle = query.trim().toLocaleLowerCase();
    if (!needle) return models;
    return models.filter((model) =>
      `${model.displayName} ${model.provider} ${model.description}`.toLocaleLowerCase().includes(needle),
    );
  }, [models, query]);

  return (
    <Dialog
      description="Choose the model for this session."
      labelledBy="model-picker-title"
      onClose={onClose}
      title="Choose a model"
    >
      <label className="searchField">
        <Icon name="search" size={16} />
        <span className="srOnly">Search models</span>
        <input
          aria-label="Search models"
          autoComplete="off"
          autoFocus
          data-initial-focus
          onChange={(event) => setQuery(event.target.value)}
          onKeyDown={(event) => {
            if (event.key === "ArrowDown") {
              event.preventDefault();
              listRef.current?.querySelector<HTMLButtonElement>("button:not(:disabled)")?.focus();
            } else if (event.key === "Enter") {
              const match = filtered.find((model) => model.selectable);
              if (query.trim() && match) { event.preventDefault(); onSelect(match.id); onClose(); }
            }
          }}
          placeholder="Search model or provider"
          type="search"
          value={query}
        />
      </label>

      <div aria-label="Available models" className="modelList" ref={listRef} role="radiogroup">
        {filtered.map((model) => {
          const selected = model.id === selectedModelId;
          return (
            <button
              aria-checked={selected}
              className="modelOption"
              data-selected={selected}
              disabled={!model.selectable}
              key={model.id}
              onClick={() => {
                onSelect(model.id);
                onClose();
              }}
              onKeyDown={(event) => {
                if (!["ArrowDown", "ArrowUp", "Home", "End"].includes(event.key)) return;
                event.preventDefault();
                const buttons = [...(listRef.current?.querySelectorAll<HTMLButtonElement>("button:not(:disabled)") ?? [])];
                const index = buttons.indexOf(event.currentTarget);
                const next = event.key === "Home" ? 0 : event.key === "End" ? buttons.length - 1 : (index + (event.key === "ArrowDown" ? 1 : -1) + buttons.length) % buttons.length;
                buttons[next]?.focus();
              }}
              role="radio"
              type="button"
            >
              <span className="modelLogo"><Icon name={model.supportsReasoning === true ? "spark" : "model"} /></span>
              <span className="modelOptionCopy">
                <span className="modelOptionTitle">
                  <strong>{model.displayName}</strong>
                  {selected ? <Chip className="selectedLabel" icon="check" intent="success">Active</Chip> : null}
                </span>
                <span>{model.description}</span>
                <span className="modelMeta">
                  <span>{model.provider}</span>
                  <span>{formatContext(model.contextWindowTokens)} context</span>
                  <span>{model.supportsTools === true ? "tools" : model.supportsTools === false ? "no tools" : "tools unknown"}</span>
                  {!model.selectable ? <span>unavailable</span> : null}
                </span>
              </span>
              <Icon className="modelChevron" name="chevron" />
            </button>
          );
        })}
        {filtered.length === 0 ? (
          <div className="emptyFilter">
            <Icon name="search" />
            <strong>No matching models</strong>
            <span>Try a provider name or clear your search.</span>
          </div>
        ) : null}
      </div>
      <div className="modelPickerFooter">
        <span>{models.filter((model) => model.selectable).length} available of {models.length} catalog models</span>
        <Button icon="refresh" onClick={onRefresh} size="small" variant="quiet">Refresh catalog</Button>
      </div>
    </Dialog>
  );
}

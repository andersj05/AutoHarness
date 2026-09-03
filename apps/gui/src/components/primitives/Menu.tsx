import { useRef, type KeyboardEvent } from "react";
import { Icon, type IconName } from "../Icon";

export interface MenuItem {
  description?: string;
  disabled?: boolean;
  icon?: IconName;
  id: string;
  label: string;
  shortcut?: string;
}

export interface MenuProps {
  activeId?: string;
  ariaLabel: string;
  emptyLabel?: string;
  items: readonly MenuItem[];
  onAction: (id: string) => void;
  onEscape?: () => void;
}

export function Menu({ activeId, ariaLabel, emptyLabel = "No actions available", items, onAction, onEscape }: MenuProps) {
  const rootRef = useRef<HTMLDivElement>(null);

  const moveFocus = (event: KeyboardEvent<HTMLButtonElement>, direction: -1 | 1 | "first" | "last") => {
    const buttons = [...(rootRef.current?.querySelectorAll<HTMLButtonElement>("button:not(:disabled)") ?? [])];
    if (buttons.length === 0) return;
    const current = buttons.indexOf(event.currentTarget);
    const next = direction === "first"
      ? 0
      : direction === "last"
        ? buttons.length - 1
        : (current + direction + buttons.length) % buttons.length;
    event.preventDefault();
    buttons[next]?.focus();
  };

  return (
    <div aria-label={ariaLabel} className="dsMenu" ref={rootRef} role="menu">
      {items.map((item) => (
        <button
          className="dsMenuItem"
          data-active={item.id === activeId}
          disabled={item.disabled}
          key={item.id}
          onClick={() => onAction(item.id)}
          onKeyDown={(event) => {
            if (event.key === "ArrowDown") moveFocus(event, 1);
            else if (event.key === "ArrowUp") moveFocus(event, -1);
            else if (event.key === "Home") moveFocus(event, "first");
            else if (event.key === "End") moveFocus(event, "last");
            else if (event.key === "Escape" && onEscape) {
              event.preventDefault();
              onEscape();
            }
          }}
          role="menuitem"
          type="button"
        >
          <span className="dsMenuIcon">{item.icon ? <Icon name={item.icon} size={16} /> : null}</span>
          <span className="dsMenuCopy"><strong>{item.label}</strong>{item.description ? <small>{item.description}</small> : null}</span>
          {item.shortcut ? <kbd>{item.shortcut}</kbd> : null}
        </button>
      ))}
      {items.length === 0 ? <p className="dsMenuEmpty">{emptyLabel}</p> : null}
    </div>
  );
}

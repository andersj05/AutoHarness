import { useMemo, useRef, useState, type KeyboardEvent } from "react";
import { Icon } from "../Icon";
import { Dialog } from "./Dialog";
import { Menu, type MenuItem } from "./Menu";

export interface CommandItem extends MenuItem {
  group?: string;
  keywords?: string;
}

export interface CommandPaletteProps {
  items: readonly CommandItem[];
  onClose: () => void;
  onSelect: (id: string) => void;
}

export function CommandPalette({ items, onClose, onSelect }: CommandPaletteProps) {
  const [query, setQuery] = useState("");
  const root = useRef<HTMLDivElement>(null);
  const filtered = useMemo(() => {
    const needle = query.trim().toLocaleLowerCase();
    if (!needle) return items;
    const matches = items.filter((item) => `${item.label} ${item.description ?? ""} ${item.group ?? ""} ${item.keywords ?? ""}`.toLocaleLowerCase().includes(needle));
    const rank = (item: CommandItem) => item.label.toLocaleLowerCase().startsWith(needle) ? 0 : item.label.toLocaleLowerCase().includes(needle) ? 1 : 2;
    return matches.sort((a, b) => rank(a) - rank(b));
  }, [items, query]);

  const focusFirstAction = (event: KeyboardEvent<HTMLInputElement>) => {
    if (event.nativeEvent.isComposing) return;
    if (event.key === "ArrowDown") {
      event.preventDefault();
      root.current?.querySelector<HTMLButtonElement>(".dsMenuItem:not(:disabled)")?.focus();
    } else if (event.key === "Enter" && query.trim()) {
      const first = filtered.find((item) => !item.disabled);
      if (first) { event.preventDefault(); onSelect(first.id); onClose(); }
    }
  };

  return (
    <Dialog labelledBy="command-palette-title" onClose={onClose} title="Search" variant="search">
      <div className="dsCommandPalette" ref={root}>
        <label className="dsCommandSearch">
          <Icon name="search" size={17} />
          <span className="srOnly">Search commands</span>
          <input aria-label="Search commands" autoComplete="off" autoFocus data-initial-focus onChange={(event) => setQuery(event.target.value)} onKeyDown={focusFirstAction} placeholder="Search sessions and commands…" type="search" value={query} />
        </label>
        <Menu
          ariaLabel="Commands"
          emptyLabel="No matching commands"
          items={filtered}
          onAction={(id) => {
            onSelect(id);
            onClose();
          }}
          onEscape={onClose}
        />
        <div aria-hidden="true" className="commandFooter"><span><kbd>↑ ↓</kbd> Navigate</span><span><kbd>Enter</kbd> Open</span><span><kbd>Esc</kbd> Close</span></div>
      </div>
    </Dialog>
  );
}

import { useMemo, useState, type KeyboardEvent } from "react";
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
  const filtered = useMemo(() => {
    const needle = query.trim().toLocaleLowerCase();
    if (!needle) return items;
    return items.filter((item) => `${item.label} ${item.description ?? ""} ${item.group ?? ""} ${item.keywords ?? ""}`.toLocaleLowerCase().includes(needle));
  }, [items, query]);

  const focusFirstAction = (event: KeyboardEvent<HTMLInputElement>) => {
    if (event.key !== "ArrowDown") return;
    event.preventDefault();
    document.querySelector<HTMLButtonElement>(".dsCommandPalette .dsMenuItem:not(:disabled)")?.focus();
  };

  return (
    <Dialog description="Search every available client action." eyebrow="Command palette" labelledBy="command-palette-title" onClose={onClose} title="Go anywhere">
      <div className="dsCommandPalette">
        <label className="dsCommandSearch">
          <Icon name="search" size={17} />
          <span className="srOnly">Search commands</span>
          <input aria-label="Search commands" autoComplete="off" autoFocus data-initial-focus onChange={(event) => setQuery(event.target.value)} onKeyDown={focusFirstAction} placeholder="Type a command" type="search" value={query} />
          <kbd>Esc</kbd>
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
      </div>
    </Dialog>
  );
}

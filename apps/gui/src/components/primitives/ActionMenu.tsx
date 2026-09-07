import { useEffect, useRef, useState } from "react";
import { Icon } from "../Icon";
import { Menu, type MenuItem } from "./Menu";

export function ActionMenu({ label, items, onAction, blocked = false }: {
  label: string;
  items: readonly MenuItem[];
  onAction: (id: string) => void;
  blocked?: boolean;
}) {
  const [open, setOpen] = useState(false);
  const root = useRef<HTMLDivElement>(null);
  const trigger = useRef<HTMLButtonElement>(null);
  useEffect(() => {
    if (blocked) setOpen(false);
  }, [blocked]);
  useEffect(() => {
    if (!open) return;
    root.current?.querySelector<HTMLButtonElement>('[role="menuitem"]:not(:disabled)')?.focus();
    const dismiss = (event: PointerEvent | FocusEvent) => {
      if (event.target instanceof Node && !root.current?.contains(event.target)) setOpen(false);
    };
    document.addEventListener("pointerdown", dismiss);
    document.addEventListener("focusin", dismiss);
    return () => {
      document.removeEventListener("pointerdown", dismiss);
      document.removeEventListener("focusin", dismiss);
    };
  }, [open]);
  const close = () => { setOpen(false); trigger.current?.focus(); };
  return <div className="actionMenu" ref={root}>
    <button aria-label={label} aria-expanded={open} aria-haspopup="menu" className="iconButton" disabled={blocked} onClick={() => setOpen((value) => !value)} ref={trigger} title={label} type="button"><Icon name="more" /></button>
    {open ? <div className="actionMenuPopover"><Menu ariaLabel={label} items={items} onAction={(id) => { close(); onAction(id); }} onEscape={close} /></div> : null}
  </div>;
}

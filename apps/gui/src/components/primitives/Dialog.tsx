import { useEffect, useRef, type PropsWithChildren, type ReactNode } from "react";
import { Button } from "./Button";

export interface DialogProps extends PropsWithChildren {
  title: string;
  description?: string;
  eyebrow?: string;
  footer?: ReactNode;
  dismissible?: boolean;
  onClose?: () => void;
  labelledBy?: string;
  authority?: "ordinary" | "permission";
}

const FOCUSABLE = "button:not([disabled]), input:not([disabled]), select:not([disabled]), textarea:not([disabled]), [tabindex]:not([tabindex='-1'])";

export function Dialog({
  title,
  description,
  eyebrow,
  footer,
  dismissible = true,
  onClose,
  children,
  labelledBy = "dialog-title",
  authority = "ordinary",
}: DialogProps) {
  const dialogRef = useRef<HTMLDivElement>(null);
  const onCloseRef = useRef(onClose);

  useEffect(() => {
    onCloseRef.current = onClose;
  }, [onClose]);

  useEffect(() => {
    const prior = document.activeElement instanceof HTMLElement ? document.activeElement : undefined;
    const root = dialogRef.current;
    (root?.querySelector<HTMLElement>("[data-initial-focus], [autofocus]") ?? root?.querySelector<HTMLElement>(FOCUSABLE))?.focus();
    const onKeyDown = (event: KeyboardEvent) => {
      if (!root) return;
      if (event.key === "Escape" && dismissible && onCloseRef.current) {
        event.preventDefault();
        onCloseRef.current();
        return;
      }
      if (event.key !== "Tab") return;
      const focusable = [...root.querySelectorAll<HTMLElement>(FOCUSABLE)];
      if (focusable.length === 0) return;
      const first = focusable[0];
      const last = focusable[focusable.length - 1];
      if (event.shiftKey && document.activeElement === first) {
        event.preventDefault();
        last?.focus();
      } else if (!event.shiftKey && document.activeElement === last) {
        event.preventDefault();
        first?.focus();
      }
    };
    document.addEventListener("keydown", onKeyDown);
    return () => {
      document.removeEventListener("keydown", onKeyDown);
      prior?.focus();
    };
  }, [dismissible]);

  return (
    <div className="dialogScrim" data-authority={authority} role="presentation">
      <div
        aria-describedby={description ? `${labelledBy}-description` : undefined}
        aria-labelledby={labelledBy}
        aria-modal="true"
        className="dialogPanel"
        data-authority={authority}
        ref={dialogRef}
        role="dialog"
      >
        <div className="dialogGlow" />
        <header className="dialogHeader">
          <div>
            {eyebrow ? <p className="eyebrow">{eyebrow}</p> : null}
            <h2 id={labelledBy}>{title}</h2>
            {description ? <p id={`${labelledBy}-description`}>{description}</p> : null}
          </div>
          {dismissible && onClose ? <Button aria-label="Close dialog" className="iconButton" onClick={onClose} variant="quiet">×</Button> : null}
        </header>
        <div className="dialogBody">{children}</div>
        {footer ? <footer className="dialogFooter">{footer}</footer> : null}
      </div>
    </div>
  );
}

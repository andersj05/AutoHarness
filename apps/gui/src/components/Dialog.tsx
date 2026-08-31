import { useEffect, useRef, type PropsWithChildren, type ReactNode } from "react";
import { Icon } from "./Icon";

interface DialogProps extends PropsWithChildren {
  title: string;
  description?: string;
  eyebrow?: string;
  footer?: ReactNode;
  dismissible?: boolean;
  onClose?: () => void;
  labelledBy?: string;
}

export function Dialog({
  title,
  description,
  eyebrow,
  footer,
  dismissible = true,
  onClose,
  children,
  labelledBy = "dialog-title",
}: DialogProps) {
  const dialogRef = useRef<HTMLDivElement>(null);
  const onCloseRef = useRef(onClose);

  useEffect(() => {
    onCloseRef.current = onClose;
  }, [onClose]);

  useEffect(() => {
    const prior = document.activeElement instanceof HTMLElement ? document.activeElement : undefined;
    const root = dialogRef.current;
    const preferred = root?.querySelector<HTMLElement>("[data-initial-focus], [autofocus]");
    const first = preferred ?? root?.querySelector<HTMLElement>(
      "button:not([disabled]), input:not([disabled]), textarea:not([disabled]), [tabindex]:not([tabindex='-1'])",
    );
    first?.focus();

    const onKeyDown = (event: KeyboardEvent) => {
      if (!root) return;
      if (event.key === "Escape" && dismissible && onCloseRef.current) {
        event.preventDefault();
        onCloseRef.current();
        return;
      }
      if (event.key !== "Tab") return;
      const focusable = [...root.querySelectorAll<HTMLElement>(
        "button:not([disabled]), input:not([disabled]), textarea:not([disabled]), [tabindex]:not([tabindex='-1'])",
      )];
      if (focusable.length === 0) return;
      const firstItem = focusable[0];
      const lastItem = focusable[focusable.length - 1];
      if (event.shiftKey && document.activeElement === firstItem) {
        event.preventDefault();
        lastItem.focus();
      } else if (!event.shiftKey && document.activeElement === lastItem) {
        event.preventDefault();
        firstItem.focus();
      }
    };
    document.addEventListener("keydown", onKeyDown);
    return () => {
      document.removeEventListener("keydown", onKeyDown);
      prior?.focus();
    };
  }, [dismissible]);

  return (
    <div className="dialogScrim" role="presentation">
      <div
        aria-describedby={description ? `${labelledBy}-description` : undefined}
        aria-labelledby={labelledBy}
        aria-modal="true"
        className="dialogPanel"
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
          {dismissible && onClose ? (
            <button aria-label="Close dialog" className="iconButton" onClick={onClose} type="button">
              <Icon name="close" />
            </button>
          ) : null}
        </header>
        <div className="dialogBody">{children}</div>
        {footer ? <footer className="dialogFooter">{footer}</footer> : null}
      </div>
    </div>
  );
}

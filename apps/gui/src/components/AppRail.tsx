import { useEffect, useRef, type CSSProperties } from "react";
import type { SessionSummary } from "../protocol";
import { Icon, type IconName } from "./Icon";

export type RouteId = "chat" | "sessions" | "providers" | "memory" | "settings";

interface AppRailProps {
  activeRoute: RouteId;
  activeSessionId?: string;
  collapsed: boolean;
  mobileViewport: boolean;
  mobileOpen: boolean;
  runtimeMode: "native" | "fixture";
  sessions: readonly SessionSummary[];
  width: number;
  onCloseMobile: () => void;
  onCreateSession: () => void;
  onOpenSession: (id: string) => void;
  onRoute: (route: RouteId) => void;
  onToggleCollapsed: () => void;
  onWidthChange: (width: number) => void;
}

const routes: readonly { id: RouteId; label: string; icon: IconName }[] = [
  { id: "chat", label: "Chat", icon: "chat" },
  { id: "sessions", label: "Sessions", icon: "sessions" },
  { id: "providers", label: "Providers", icon: "providers" },
  { id: "memory", label: "Memory", icon: "memory" },
];

export function AppRail({
  activeRoute,
  activeSessionId,
  collapsed,
  mobileViewport,
  mobileOpen,
  runtimeMode,
  sessions,
  width,
  onCloseMobile,
  onCreateSession,
  onOpenSession,
  onRoute,
  onToggleCollapsed,
  onWidthChange,
}: AppRailProps) {
  const closeButtonRef = useRef<HTMLButtonElement>(null);
  const closeMobileRef = useRef(onCloseMobile);
  const railRef = useRef<HTMLElement>(null);

  useEffect(() => {
    closeMobileRef.current = onCloseMobile;
  }, [onCloseMobile]);

  useEffect(() => {
    if (mobileViewport && !mobileOpen) railRef.current?.setAttribute("inert", "");
    else railRef.current?.removeAttribute("inert");
  }, [mobileOpen, mobileViewport]);

  useEffect(() => {
    if (!mobileViewport || !mobileOpen) return;
    const prior = document.activeElement instanceof HTMLElement ? document.activeElement : undefined;
    closeButtonRef.current?.focus();
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        event.preventDefault();
        closeMobileRef.current();
        return;
      }
      if (event.key !== "Tab" || !railRef.current) return;
      const focusable = [...railRef.current.querySelectorAll<HTMLElement>(
        "button:not([disabled]), input:not([disabled]), [tabindex]:not([tabindex='-1'])",
      )];
      const first = focusable[0];
      const last = focusable[focusable.length - 1];
      if (!first || !last) return;
      if (event.shiftKey && document.activeElement === first) {
        event.preventDefault();
        last.focus();
      } else if (!event.shiftKey && document.activeElement === last) {
        event.preventDefault();
        first.focus();
      }
    };
    document.addEventListener("keydown", onKeyDown);
    return () => {
      document.removeEventListener("keydown", onKeyDown);
      queueMicrotask(() => prior?.focus());
    };
  }, [mobileOpen, mobileViewport]);

  const navigate = (route: RouteId) => {
    onRoute(route);
    onCloseMobile();
  };

  return (
    <aside
      aria-hidden={mobileViewport && !mobileOpen ? true : undefined}
      aria-label={mobileViewport ? "Navigation drawer" : undefined}
      aria-modal={mobileViewport && mobileOpen ? true : undefined}
      className="appRail"
      data-collapsed={collapsed}
      data-mobile-open={mobileOpen}
      ref={railRef}
      role={mobileViewport ? "dialog" : undefined}
      style={{ "--rail-size": `${width}px` } as CSSProperties}
    >
      <div className="railBrand">
        <div className="brandGlyph" aria-hidden="true">
          <span />
          <span />
          <span />
        </div>
        <div className="brandCopy">
          <strong>AutoHarness</strong>
          <span>{runtimeMode === "fixture" ? "browser fixture" : "agent workspace"}</span>
        </div>
        <button
          aria-label={collapsed ? "Expand navigation" : "Collapse navigation"}
          className="iconButton railCollapse"
          onClick={onToggleCollapsed}
          type="button"
        >
          <Icon name="panel-left" />
        </button>
        {mobileOpen ? (
          <button aria-label="Close navigation" className="iconButton mobileRailClose" onClick={onCloseMobile} ref={closeButtonRef} type="button">
            <Icon name="close" />
          </button>
        ) : null}
      </div>

      <button aria-label="Create new session" className="newSessionButton" onClick={onCreateSession} title="Create new session" type="button">
        <Icon name="new" />
        <span>New session</span>
        <kbd>Ctrl N</kbd>
      </button>

      <nav aria-label="Primary" className="routeNav">
        {routes.map((route) => (
          <button
            aria-label={route.label}
            aria-current={activeRoute === route.id ? "page" : undefined}
            className="routeButton"
            data-active={activeRoute === route.id}
            key={route.id}
            onClick={() => navigate(route.id)}
            title={route.label}
            type="button"
          >
            <Icon name={route.icon} />
            <span>{route.label}</span>
          </button>
        ))}
      </nav>

      <section aria-labelledby="recent-sessions-label" className="recentSessions">
        <div className="railSectionHeading">
          <span id="recent-sessions-label">Recent</span>
          <button aria-label="Search sessions" className="quietIconButton" onClick={() => navigate("sessions")} type="button">
            <Icon name="search" size={15} />
          </button>
        </div>
        <div className="sessionRailList">
          {sessions.slice(0, 5).map((session) => (
            <button
              aria-current={activeSessionId === session.id ? "true" : undefined}
              className="sessionRailItem"
              data-active={activeSessionId === session.id}
              key={session.id}
              onClick={() => {
                onOpenSession(session.id);
                navigate("chat");
              }}
              title={session.title}
              type="button"
            >
              <span className="sessionMarker" />
              <span>{session.title}</span>
            </button>
          ))}
        </div>
      </section>

      <div className="railFooter">
        <button
          aria-label="Settings"
          aria-current={activeRoute === "settings" ? "page" : undefined}
          className="routeButton"
          data-active={activeRoute === "settings"}
          onClick={() => navigate("settings")}
          title="Settings"
          type="button"
        >
          <Icon name="settings" />
          <span>Settings</span>
        </button>
        <div className="profileSummary">
          <span className="avatar" aria-hidden="true">A</span>
          <span className="profileCopy">
            <strong>{runtimeMode === "fixture" ? "Browser fixture" : "Local workspace"}</strong>
            <small>{runtimeMode === "fixture" ? "Simulated state only" : "Private by default"}</small>
          </span>
          <span className="onlineDot" data-fixture={runtimeMode === "fixture"} title={runtimeMode === "fixture" ? "Fixture preview" : "Runtime ready"} />
        </div>
      </div>
      {!collapsed && !mobileViewport ? (
        <button
          aria-label="Resize navigation"
          aria-orientation="vertical"
          aria-valuemax={340}
          aria-valuemin={208}
          aria-valuenow={width}
          className="railResizeHandle"
          onKeyDown={(event) => {
            if (event.key === "ArrowLeft") {
              event.preventDefault();
              onWidthChange(Math.max(208, width - (event.shiftKey ? 24 : 8)));
            } else if (event.key === "ArrowRight") {
              event.preventDefault();
              onWidthChange(Math.min(340, width + (event.shiftKey ? 24 : 8)));
            } else if (event.key === "Home") {
              event.preventDefault();
              onWidthChange(208);
            } else if (event.key === "End") {
              event.preventDefault();
              onWidthChange(340);
            }
          }}
          onPointerDown={(event) => event.currentTarget.setPointerCapture(event.pointerId)}
          onPointerMove={(event) => {
            if (!event.currentTarget.hasPointerCapture(event.pointerId)) return;
            onWidthChange(Math.min(340, Math.max(208, Math.round(event.clientX))));
          }}
          onPointerUp={(event) => event.currentTarget.releasePointerCapture(event.pointerId)}
          role="separator"
          type="button"
        ><span /></button>
      ) : null}
    </aside>
  );
}

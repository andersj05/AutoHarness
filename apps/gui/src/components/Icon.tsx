import type { SVGProps } from "react";

export type IconName =
  | "arrow-up"
  | "bolt"
  | "branch"
  | "chat"
  | "check"
  | "chevron"
  | "close"
  | "command"
  | "copy"
  | "database"
  | "download"
  | "inspect"
  | "memory"
  | "menu"
  | "model"
  | "new"
  | "panel-left"
  | "panel-right"
  | "refresh"
  | "search"
  | "sessions"
  | "settings"
  | "shield"
  | "spark"
  | "stop"
  | "terminal"
  | "tool"
  | "warning";

const paths: Record<IconName, JSX.Element> = {
  "arrow-up": <path d="m5 12 7-7 7 7M12 5v14" />,
  bolt: <path d="m13 2-8 12h7l-1 8 8-12h-7l1-8Z" />,
  branch: <path d="M6 3v12a4 4 0 0 0 4 4h5M18 5a2 2 0 1 1-4 0 2 2 0 0 1 4 0ZM20 19a2 2 0 1 1-4 0 2 2 0 0 1 4 0Z" />,
  chat: <path d="M20 14a4 4 0 0 1-4 4H8l-5 3V7a4 4 0 0 1 4-4h9a4 4 0 0 1 4 4v7Z" />,
  check: <path d="m5 12 4 4L19 6" />,
  chevron: <path d="m9 18 6-6-6-6" />,
  close: <path d="M6 6l12 12M18 6 6 18" />,
  command: <path d="M9 6a3 3 0 1 1-3-3v15a3 3 0 1 1 3-3H6h12a3 3 0 1 1-3 3V6a3 3 0 1 1 3 3H6" />,
  copy: <path d="M8 8h11v11H8zM5 16H3V3h13v2" />,
  database: <path d="M20 6c0 1.7-3.6 3-8 3S4 7.7 4 6s3.6-3 8-3 8 1.3 8 3Zm0 0v6c0 1.7-3.6 3-8 3s-8-1.3-8-3V6m16 6v6c0 1.7-3.6 3-8 3s-8-1.3-8-3v-6" />,
  download: <path d="M12 3v12m0 0 5-5m-5 5-5-5M5 20h14" />,
  inspect: <path d="M4 4h16v16H4zM14 4v16M7 8h4M7 12h4" />,
  memory: <path d="M9 4V2m6 2V2M9 22v-2m6 2v-2M4 9H2m2 6H2m20-6h-2m2 6h-2M6 4h12a2 2 0 0 1 2 2v12a2 2 0 0 1-2 2H6a2 2 0 0 1-2-2V6a2 2 0 0 1 2-2Zm3 5h6v6H9z" />,
  menu: <path d="M4 7h16M4 12h16M4 17h16" />,
  model: <path d="m12 3 8 4.5v9L12 21l-8-4.5v-9L12 3Zm0 0v9m8-4.5-8 4.5-8-4.5M12 12v9" />,
  new: <path d="M12 5v14M5 12h14" />,
  "panel-left": <path d="M4 4h16v16H4zM9 4v16" />,
  "panel-right": <path d="M4 4h16v16H4zM15 4v16" />,
  refresh: <path d="M20 7v5h-5M4 17v-5h5m9.5-3A7 7 0 0 0 6.8 6.2L4 9m16 6-2.8 2.8A7 7 0 0 1 5.5 15" />,
  search: <path d="m21 21-4.4-4.4M19 11a8 8 0 1 1-16 0 8 8 0 0 1 16 0Z" />,
  sessions: <path d="M6 7h12M6 12h12M6 17h8M3 4h18v16H3z" />,
  settings: <path d="M12 15.5a3.5 3.5 0 1 0 0-7 3.5 3.5 0 0 0 0 7Zm7.4-2.1 1.4 1.1-2 3.5-1.8-.7a8 8 0 0 1-2 1.2L14.7 21h-4l-.3-2.5a8 8 0 0 1-2-1.2l-1.8.7-2-3.5L6 13.4a8 8 0 0 1 0-2.8L4.6 9.5l2-3.5 1.8.7a8 8 0 0 1 2-1.2l.3-2.5h4l.3 2.5a8 8 0 0 1 2 1.2l1.8-.7 2 3.5-1.4 1.1a8 8 0 0 1 0 2.8Z" />,
  shield: <path d="M12 3 5 6v5c0 4.6 2.8 8.4 7 10 4.2-1.6 7-5.4 7-10V6l-7-3Zm-3 9 2 2 4-5" />,
  spark: <path d="m12 2 1.4 5.6L19 9l-5.6 1.4L12 16l-1.4-5.6L5 9l5.6-1.4L12 2Zm6 13 .7 2.3L21 18l-2.3.7L18 21l-.7-2.3L15 18l2.3-.7L18 15Z" />,
  stop: <path d="M7 7h10v10H7z" />,
  terminal: <path d="m4 7 4 4-4 4m7 0h9M3 3h18v18H3z" />,
  tool: <path d="M14.5 6.5a4 4 0 0 0-5-5L12 4l-3 3-2.5-2.5a4 4 0 0 0 5 5L20 18l-2 2-8.5-8.5" />,
  warning: <path d="M12 3 2.5 20h19L12 3Zm0 6v5m0 3v.01" />,
};

interface IconProps extends SVGProps<SVGSVGElement> {
  name: IconName;
  size?: number;
}

export function Icon({ name, size = 18, ...props }: IconProps) {
  return (
    <svg
      aria-hidden="true"
      fill="none"
      height={size}
      viewBox="0 0 24 24"
      width={size}
      stroke="currentColor"
      strokeLinecap="round"
      strokeLinejoin="round"
      strokeWidth="1.7"
      {...props}
    >
      {paths[name]}
    </svg>
  );
}

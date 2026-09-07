import { memo, type ReactNode } from "react";
import Markdown from "react-markdown";
import remarkGfm from "remark-gfm";

/** Provider text never creates active links, remote images, or executable HTML. */
export const MessageContent = memo(function MessageContent({ text, highlight, plain = false }: { text: string; highlight?: string; plain?: boolean }) {
  if (highlight) {
    const needle = highlight.toLocaleLowerCase();
    const lower = text.toLocaleLowerCase();
    const parts: ReactNode[] = [];
    let cursor = 0;
    let match = lower.indexOf(needle);
    while (match >= 0) {
      parts.push(text.slice(cursor, match), <mark key={match}>{text.slice(match, match + highlight.length)}</mark>);
      cursor = match + highlight.length;
      match = lower.indexOf(needle, cursor);
    }
    parts.push(text.slice(cursor));
    return <div className="messagePlainText">{parts}</div>;
  }
  if (plain) return <div className="messagePlainText">{text}</div>;
  return <Markdown remarkPlugins={[remarkGfm]} components={{
    a: ({ children, href }) => <span className="messageLink">{children}{href ? <span className="messageLinkTarget"> ({href})</span> : null}</span>,
    img: ({ alt }) => <span className="messageImage">[Image{alt ? `: ${alt}` : ""}]</span>,
    h1: ({ children }) => <h3>{children}</h3>,
    h2: ({ children }) => <h3>{children}</h3>,
    table: ({ children }) => <div className="messageTable"><table>{children}</table></div>,
  }}>{text}</Markdown>;
});

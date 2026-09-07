import { Children, isValidElement, memo, useRef, useState, type ReactNode } from "react";
import Markdown from "react-markdown";
import remarkGfm from "remark-gfm";
import { Icon } from "./Icon";

function CodeBlock({ children }: { children: ReactNode }) {
  const content = useRef<HTMLPreElement>(null);
  const [status, setStatus] = useState("");
  const code = Children.toArray(children).find((child) => isValidElement<{ className?: string }>(child));
  const language = isValidElement<{ className?: string }>(code)
    ? code.props.className?.match(/(?:^|\s)language-([\w+-]+)/)?.[1] ?? ""
    : "";
  const copy = async () => {
    try {
      if (!navigator.clipboard?.writeText) throw new Error("Clipboard unavailable");
      await navigator.clipboard.writeText(content.current?.textContent ?? "");
      setStatus("Copied");
    } catch {
      setStatus("Could not copy. Select the code to copy it manually.");
    }
  };
  return <div className="messageCodeBlock">
    <div className="codeToolbar"><span>{language}</span><button aria-label="Copy code" onClick={() => void copy()} type="button"><Icon name={status === "Copied" ? "check" : "copy"} size={14} />{status === "Copied" ? "Copied" : "Copy"}</button></div>
    <pre ref={content}>{children}</pre>
    <span className={status === "Copied" ? "srOnly" : "codeCopyStatus"} role="status">{status}</span>
  </div>;
}

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
    pre: ({ children }) => <CodeBlock>{children}</CodeBlock>,
    table: ({ children }) => <div className="messageTable"><table>{children}</table></div>,
  }}>{text}</Markdown>;
});

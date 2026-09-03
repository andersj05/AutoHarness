import { useEffect, useLayoutEffect, useRef, useState, type ReactNode, type RefObject } from "react";
import type { TranscriptItem } from "../protocol";

const ESTIMATED_ITEM_HEIGHT = 190;
const WINDOW_SIZE = 36;
const OVERSCAN = 8;

interface VirtualTranscriptProps {
  activeIndex?: number;
  items: readonly TranscriptItem[];
  renderItem: (item: TranscriptItem, index: number) => ReactNode;
  scrollRef: RefObject<HTMLDivElement>;
  sessionId?: string;
}

function windowStart(index: number, itemCount: number): number {
  const maximum = Math.max(0, itemCount - WINDOW_SIZE);
  return Math.max(0, Math.min(maximum, index - OVERSCAN));
}

export function VirtualTranscript({ activeIndex, items, renderItem, scrollRef, sessionId }: VirtualTranscriptProps) {
  const [start, setStart] = useState(() => Math.max(0, items.length - WINDOW_SIZE));
  const trackRef = useRef<HTMLDivElement>(null);
  const previousCountRef = useRef(items.length);

  useEffect(() => {
    setStart(Math.max(0, items.length - WINDOW_SIZE));
    previousCountRef.current = items.length;
  }, [sessionId]);

  useEffect(() => {
    const previousCount = previousCountRef.current;
    previousCountRef.current = items.length;
    if (items.length <= previousCount) return;
    setStart((current) => (
      current + WINDOW_SIZE + OVERSCAN >= previousCount
        ? Math.max(0, items.length - WINDOW_SIZE)
        : current
    ));
  }, [items.length]);

  useEffect(() => {
    const scroll = scrollRef.current;
    if (!scroll) return;
    const updateWindow = () => {
      const trackTop = trackRef.current?.offsetTop ?? 0;
      const relativeTop = Math.max(0, scroll.scrollTop - trackTop);
      const approximateIndex = Math.floor(relativeTop / ESTIMATED_ITEM_HEIGHT);
      setStart((current) => {
        const next = windowStart(approximateIndex, items.length);
        return current === next ? current : next;
      });
    };
    scroll.addEventListener("scroll", updateWindow, { passive: true });
    return () => scroll.removeEventListener("scroll", updateWindow);
  }, [items.length, scrollRef]);

  useLayoutEffect(() => {
    if (activeIndex === undefined) return;
    setStart(windowStart(activeIndex, items.length));
  }, [activeIndex, items.length]);

  useLayoutEffect(() => {
    if (activeIndex === undefined) return;
    const target = trackRef.current?.querySelector<HTMLElement>(`[data-transcript-index="${activeIndex}"]`);
    target?.scrollIntoView?.({ block: "center" });
  }, [activeIndex, start]);

  const end = Math.min(items.length, start + WINDOW_SIZE);
  const visible = items.slice(start, end);
  return (
    <section aria-label="Conversation transcript" className="transcript virtualTranscript" ref={trackRef} tabIndex={-1}>
      {start > 0 ? <div aria-hidden="true" className="virtualTranscriptSpacer" data-position="before" style={{ height: start * ESTIMATED_ITEM_HEIGHT }} /> : null}
      {visible.map((item, offset) => {
        const index = start + offset;
        return (
          <div
            className="virtualTranscriptItem"
            data-search-active={index === activeIndex}
            data-transcript-index={index}
            data-virtual-transcript-item
            key={item.id}
          >
            {renderItem(item, index)}
          </div>
        );
      })}
      {end < items.length ? <div aria-hidden="true" className="virtualTranscriptSpacer" data-position="after" style={{ height: (items.length - end) * ESTIMATED_ITEM_HEIGHT }} /> : null}
    </section>
  );
}

import { useState, type CSSProperties, type ReactNode, type UIEvent } from "react";

export interface VirtualListProps<T> {
  ariaLabel: string;
  height: number;
  items: readonly T[];
  itemKey: (item: T) => string;
  overscan?: number;
  renderItem: (item: T, index: number) => ReactNode;
  rowHeight: number;
}

export function VirtualList<T>({ ariaLabel, height, itemKey, items, overscan = 3, renderItem, rowHeight }: VirtualListProps<T>) {
  const [scrollTop, setScrollTop] = useState(0);
  const start = Math.max(0, Math.floor(scrollTop / rowHeight) - overscan);
  const visibleCount = Math.ceil(height / rowHeight) + overscan * 2;
  const end = Math.min(items.length, start + visibleCount);
  const visible = items.slice(start, end);
  return (
    <div
      aria-label={ariaLabel}
      className="dsVirtualList"
      onScroll={(event: UIEvent<HTMLDivElement>) => setScrollTop(event.currentTarget.scrollTop)}
      role="list"
      style={{ height }}
      tabIndex={0}
    >
      <div className="dsVirtualListTrack" style={{ height: items.length * rowHeight }}>
        {visible.map((item, offset) => {
          const index = start + offset;
          return (
            <div
              aria-posinset={index + 1}
              aria-setsize={items.length}
              className="dsVirtualListItem"
              key={itemKey(item)}
              role="listitem"
              style={{ "--virtual-offset": `${index * rowHeight}px`, height: rowHeight } as CSSProperties}
            >
              {renderItem(item, index)}
            </div>
          );
        })}
      </div>
    </div>
  );
}

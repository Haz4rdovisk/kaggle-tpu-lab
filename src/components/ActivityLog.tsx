import { useEffect, useRef } from "react";
import { ListDashes } from "@phosphor-icons/react";
import type { ActivityNote } from "../types/session";
import { formatClock } from "../lib/format";

interface Props {
  activity: ActivityNote[];
}

export default function ActivityLog({ activity }: Props) {
  const boxRef = useRef<HTMLDivElement>(null);
  const topCountRef = useRef(0);

  // Keep the newest entries in view unless the user has scrolled away.
  useEffect(() => {
    const el = boxRef.current;
    if (!el) return;
    const nearTop = el.scrollTop < 24;
    if (nearTop || topCountRef.current <= activity.length) {
      el.scrollTop = 0;
    }
    topCountRef.current = activity.length;
  }, [activity.length]);

  return (
    <section className="activity" aria-label="Activity log">
      <div className="card-head activity-head">
        <span className="card-title">ACTIVITY</span>
        <span className="card-sub">{activity.length} events</span>
      </div>
      <div className="activity-list" ref={boxRef}>
        {activity.length === 0 && (
          <div className="activity-empty">
            <ListDashes size={18} aria-hidden />
            <span>No events yet this session.</span>
          </div>
        )}
        {activity.map((n, i) => (
          <div className="activity-row" key={`${n.ts}-${i}`}>
            <span className={`kind-dot tone-${n.kind}`} aria-hidden />
            <time className="activity-ts">{formatClock(n.ts)}</time>
            <span className="activity-text">{n.text}</span>
          </div>
        ))}
      </div>
    </section>
  );
}

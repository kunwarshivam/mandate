import { cn } from "@/lib/utils";
import type { Point } from "@/fixtures/market";

/**
 * Today's equity as one line, scaled to its own range so a quiet day still shows its shape, with
 * the daily loss limit as the pale volt region below it (DEC-515): the region rises into view as the
 * agent nears its limit, and sits as a sliver at the foot while the limit is far below. A line
 * drawn far above a distant dashed rule read as broken. The dashed rule marks the limit only once it
 * is inside the line's range: below it, a rule under the lowest point read as the agent touching its
 * limit (critique C-11). It stretches to its column and keeps its strokes hairline-true.
 * Decoration: `label` says it.
 */
export function Sparkline({
  points,
  limit,
  label,
  width = 160,
  height = 40,
  className,
}: {
  points: Point[];
  limit: number | null;
  label: string;
  width?: number;
  height?: number;
  className?: string;
}) {
  if (points.length < 2) return null;
  const values = points.map((p) => p.value);
  const lo = Math.min(...values);
  const hi = Math.max(...values);
  const span = hi - lo || Math.max(Math.abs(hi) * 0.002, 1);
  const t0 = points[0].time;
  const t1 = points[points.length - 1].time;
  const x = (t: number) => ((t - t0) / (t1 - t0 || 1)) * (width - 2) + 1;
  const y = (v: number) => height - 3 - ((v - lo) / span) * (height - 6);
  const path = points.map((p) => `${x(p.time).toFixed(1)},${y(p.value).toFixed(1)}`).join(" ");
  const SLIVER = 2;
  const limitTop = limit === null ? null : Math.min(Math.max(y(limit), 0), height - SLIVER);
  const limitBelowRange = limit !== null && limit < lo;
  return (
    <svg
      data-slot="sparkline"
      role="img"
      aria-label={label}
      viewBox={`0 0 ${width} ${height}`}
      preserveAspectRatio="none"
      className={cn("block h-10 w-full max-w-40", className)}
    >
      {limitTop !== null ? (
        <>
          <rect data-slot="sparkline-limit-band" x={0} y={limitTop} width={width} height={height - limitTop} className="fill-mandate" />
          {limitBelowRange ? null : (
            <line data-slot="sparkline-limit" x1={0} x2={width} y1={limitTop} y2={limitTop} className="stroke-mandate-marker" strokeWidth={1} strokeDasharray="3 3" vectorEffect="non-scaling-stroke" />
          )}
        </>
      ) : null}
      <polyline points={path} fill="none" className="stroke-foreground" strokeWidth={1.5} strokeLinejoin="round" strokeLinecap="round" vectorEffect="non-scaling-stroke" />
    </svg>
  );
}

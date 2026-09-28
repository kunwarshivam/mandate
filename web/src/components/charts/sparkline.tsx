import { cn } from "@/lib/utils";
import type { Point } from "@/fixtures/market";

/**
 * Today's equity as one line, with the daily loss limit as a brass rule. The vertical range
 * always includes the limit, so the gap between them is to scale. It stretches to its column and
 * keeps its strokes hairline-true. Decoration: `label` says it.
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
  const lo = Math.min(...values, limit ?? Number.POSITIVE_INFINITY);
  const hi = Math.max(...values, limit ?? Number.NEGATIVE_INFINITY);
  const span = hi - lo || 1;
  const t0 = points[0].time;
  const t1 = points[points.length - 1].time;
  const x = (t: number) => ((t - t0) / (t1 - t0 || 1)) * (width - 2) + 1;
  const y = (v: number) => height - 3 - ((v - lo) / span) * (height - 6);
  const path = points.map((p) => `${x(p.time).toFixed(1)},${y(p.value).toFixed(1)}`).join(" ");
  return (
    <svg
      data-slot="sparkline"
      role="img"
      aria-label={label}
      viewBox={`0 0 ${width} ${height}`}
      preserveAspectRatio="none"
      className={cn("block h-10 w-full max-w-40", className)}
    >
      {limit !== null ? (
        <line
          data-slot="sparkline-limit"
          x1={0}
          x2={width}
          y1={y(limit)}
          y2={y(limit)}
          className="stroke-mandate-marker"
          strokeWidth={1}
          strokeDasharray="3 3"
          vectorEffect="non-scaling-stroke"
        />
      ) : null}
      <polyline points={path} fill="none" className="stroke-foreground" strokeWidth={1.5} strokeLinejoin="round" strokeLinecap="round" vectorEffect="non-scaling-stroke" />
    </svg>
  );
}

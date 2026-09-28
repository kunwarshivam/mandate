import type { Point } from "@/fixtures/market";

/**
 * Today's equity as one line, with the daily loss limit as a marigold rule. The vertical range
 * always includes the limit, so the gap between them is to scale. Decoration: `label` says it.
 */
export function Sparkline({ points, limit, label, width = 160, height = 40 }: { points: Point[]; limit: number | null; label: string; width?: number; height?: number }) {
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
    <svg data-slot="sparkline" role="img" aria-label={label} viewBox={`0 0 ${width} ${height}`} width={width} height={height} className="block max-w-full">
      {limit !== null ? <line data-slot="sparkline-limit" x1={0} x2={width} y1={y(limit)} y2={y(limit)} className="stroke-marigold" strokeWidth={2} /> : null}
      <polyline points={path} fill="none" className="stroke-foreground" strokeWidth={1.5} strokeLinejoin="round" />
    </svg>
  );
}

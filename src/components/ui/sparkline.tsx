import { cn } from "@/lib/utils";

/** The drawing's size, in viewBox units (it stretches to its box). */
const W = 160;
const H = 36;

/**
 * A tiny area chart for stat cards (CPU, memory). Values are 0 to 100; the last point is
 * marked. Uses the jewel, or the warning colour when `warn`.
 */
export function Sparkline({ values, warn, className }: { values: number[]; warn?: boolean; className?: string }) {
  const pts = values.length > 1 ? values : [values[0] ?? 0, values[0] ?? 0];
  const step = W / (pts.length - 1);
  const y = (v: number) => (H - (Math.max(0, Math.min(100, v)) / 100) * (H - 3) - 1.5).toFixed(1);
  const line = pts.map((v, i) => `${(i * step).toFixed(1)},${y(v)}`).join(" ");
  const color = warn ? "var(--warning)" : "var(--jewel)";
  return (
    <svg viewBox={`0 0 ${String(W)} ${String(H)}`} preserveAspectRatio="none" aria-hidden className={cn("h-9 w-full", className)}>
      <polyline points={`0,${String(H)} ${line} ${String(W)},${String(H)}`} style={{ fill: color }} fillOpacity={0.12} stroke="none" />
      <polyline points={line} fill="none" style={{ stroke: color }} strokeWidth={1.5} vectorEffect="non-scaling-stroke" />
    </svg>
  );
}

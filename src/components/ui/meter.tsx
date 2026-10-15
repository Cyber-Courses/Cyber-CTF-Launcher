import { cn } from "@/lib/utils";

/** A thin usage bar: the jewel when calm, warm over 75%, fail over 90%. */
export function Meter({ value, className }: { value: number; className?: string }) {
  const v = Math.max(0, Math.min(100, value));
  const tone = v > 90 ? "meter-fill-fail" : v > 75 ? "meter-fill-warn" : "meter-fill";
  return (
    <div className={cn("h-1.5 w-full overflow-hidden rounded-full bg-border", className)}>
      <div className={cn("h-full rounded-full transition-[width] duration-500", tone)} style={{ width: `${String(v)}%` }} />
    </div>
  );
}

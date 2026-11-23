import { cn } from "@/lib/class-names";

/** A softly pulsing placeholder block for loading states. */
export function Skeleton({ className }: { className?: string }) {
  return <div className={cn("animate-pulse rounded-sm bg-glass-2", className)} />;
}

import type { HTMLAttributes } from "react";
import { cn } from "@/lib/utils";

type Variant = "default" | "success" | "warning" | "destructive" | "accent" | "outline";

const VARIANTS: Record<Variant, string> = {
  default: "text-muted-foreground shadow-[inset_0_0_0_1px_var(--input)]",
  outline: "text-muted-foreground shadow-[inset_0_0_0_1px_var(--input)]",
  success: "text-success shadow-[inset_0_0_0_1px_color-mix(in_oklab,var(--success)_35%,transparent)]",
  warning: "text-warning shadow-[inset_0_0_0_1px_color-mix(in_oklab,var(--warning)_35%,transparent)]",
  destructive: "text-destructive shadow-[inset_0_0_0_1px_color-mix(in_oklab,var(--destructive)_35%,transparent)]",
  accent: "text-jewel-text shadow-[inset_0_0_0_1px_rgb(var(--jewel-rgb)/0.4)]",
};

/** A mono pill for a status or label. Set `dot` for a leading status dot (uses currentColor). */
export function Badge({
  variant = "default",
  dot = false,
  className,
  children,
  ...props
}: HTMLAttributes<HTMLSpanElement> & { variant?: Variant; dot?: boolean }) {
  return (
    <span
      className={cn(
        "inline-flex items-center gap-1.5 rounded-full px-2 py-0.5 font-mono text-[0.625rem] font-medium whitespace-nowrap",
        VARIANTS[variant],
        className,
      )}
      {...props}
    >
      {dot && <span className="size-1.5 rounded-full bg-current" />}
      {children}
    </span>
  );
}

/** Lab level: beginner (green), intermediate (amber), advanced (red). Darker in Light. */
export function LevelBadge({ level, children }: { level: number; children: React.ReactNode }) {
  const tone = level === 1 ? "text-success" : level === 2 ? "text-warning" : "text-destructive";
  return (
    <span className={cn("inline-flex rounded-full px-2 py-0.5 font-mono text-[0.625rem] font-medium shadow-[inset_0_0_0_1px_var(--input)]", tone)}>
      {children}
    </span>
  );
}

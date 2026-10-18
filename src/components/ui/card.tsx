import type { HTMLAttributes } from "react";
import { cn } from "@/lib/utils";

/** The family Card: a panel surface (gradient with a lit top edge). Same look as Panel. */
export function Card({ className, ...props }: HTMLAttributes<HTMLDivElement>) {
  return <div className={cn("surface-panel rounded-panel text-card-foreground", className)} {...props} />;
}

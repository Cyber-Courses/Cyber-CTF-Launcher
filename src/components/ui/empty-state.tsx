import type { ReactNode } from "react";
import { Icon, type IconName } from "@/components/ui/icon";
import { cn } from "@/lib/utils";

/** Centered empty or placeholder state inside a panel: icon, serif title, description, action. */
export function EmptyState({
  icon,
  title,
  description,
  action,
  className,
}: {
  icon: IconName;
  title: string;
  description?: string;
  action?: ReactNode;
  className?: string;
}) {
  return (
    <div className={cn("surface-panel flex flex-col items-center justify-center rounded-panel px-6 py-12 text-center", className)}>
      <div className="flex size-10 items-center justify-center rounded-control bg-glass-2 text-jewel-text shadow-[inset_0_0_0_1px_var(--input)]">
        <Icon name={icon} className="size-5" />
      </div>
      <h3 className="serif-title mt-4 text-[1.5rem] text-foreground">{title}</h3>
      {description && <p className="mt-1.5 max-w-sm text-[0.875rem] text-muted-foreground">{description}</p>}
      {action && <div className="mt-5">{action}</div>}
    </div>
  );
}

import { useId, type ReactNode } from "react";
import { AvatarGroup, type AvatarItem } from "@/components/ui/avatar";
import { Icon, type IconName } from "@/components/ui/icon";
import { cn } from "@/lib/utils";

/**
 * What a page or panel shows while it has nothing yet, the same everywhere: an icon tile, a short
 * serif title (an h3, as the panels' titles under the page's h1), one sentence on what goes here,
 * the providers it works with as a group of avatars (`marks`: their logos, named on hover and for
 * screen readers, never as text), then the one action that fills it (a primary Button,
 * `size="sm"`) and, when there is one, a secondary way out (a `link` Button). A region named by
 * its title, so it reads as one block.
 */
export function EmptyState({
  icon,
  title,
  description,
  marks,
  action,
  secondary,
  className,
}: {
  icon: IconName;
  title: string;
  description?: string;
  marks?: AvatarItem[];
  action?: ReactNode;
  secondary?: ReactNode;
  className?: string;
}) {
  const titleId = useId();
  return (
    <section
      aria-labelledby={titleId}
      className={cn("surface-panel flex flex-col items-center justify-center rounded-panel px-6 py-12 text-center", className)}
    >
      <div className="flex size-10 items-center justify-center rounded-control bg-glass-2 text-jewel-text shadow-[inset_0_0_0_1px_var(--input)]">
        <Icon name={icon} className="size-5" />
      </div>
      <h3 id={titleId} className="serif-title mt-4 text-[1.5rem] text-foreground">
        {title}
      </h3>
      {description && <p className="mt-1.5 max-w-sm text-[0.875rem] text-muted-foreground">{description}</p>}
      {marks && marks.length > 0 && <AvatarGroup items={marks} className="mt-4" />}
      {(action || secondary) && (
        <div className="mt-5 flex flex-col items-center gap-3">
          {action}
          {secondary}
        </div>
      )}
    </section>
  );
}

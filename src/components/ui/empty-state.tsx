import { useId, type ReactNode } from "react";
import { Icon, type IconName } from "@/components/ui/icon";
import { cn } from "@/lib/utils";

/** A provider the page works with, shown by its mark (a `LogoTile`, size sm) and its name; with
 *  `select`, a shortcut into the action for that provider, named by its `label`. */
export type EmptyStateMark = { name: string; logo: ReactNode; select?: { label: string; onSelect: () => void } };

const CHIP =
  "inline-flex h-8 items-center gap-2 rounded-full bg-glass py-1 pr-3 pl-1 text-[0.75rem] text-muted-foreground shadow-[inset_0_0_0_1px_var(--border)]";

/**
 * What a page or panel shows while it has nothing yet, the same everywhere: an icon tile, a short
 * serif title (an h3, as the panels' titles under the page's h1), one sentence on what goes here,
 * the providers it works with (`marks`, quiet chips: the action stays the one call to it), then
 * the one action that fills it (a primary Button, `size="sm"`) and, when there is one, a secondary
 * way out (a `link` Button). A region named by its title, so it reads as one block. Marks with no
 * shortcut are decoration (the sentence names what matters), hidden from assistive tech.
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
  marks?: EmptyStateMark[];
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
      {marks && marks.length > 0 && <Marks marks={marks} />}
      {(action || secondary) && (
        <div className="mt-5 flex flex-col items-center gap-3">
          {action}
          {secondary}
        </div>
      )}
    </section>
  );
}

function Marks({ marks }: { marks: EmptyStateMark[] }) {
  const decorative = marks.every((m) => !m.select);
  return (
    <ul aria-hidden={decorative || undefined} className="mt-4 flex max-w-lg flex-wrap justify-center gap-2">
      {marks.map(({ name, logo, select }) => (
        <li key={name}>
          {select ? (
            <button
              type="button"
              aria-label={select.label}
              onClick={select.onSelect}
              className={cn(CHIP, "transition-colors hover:bg-glass-2 hover:text-foreground")}
            >
              {logo}
              {name}
            </button>
          ) : (
            <span className={CHIP}>
              {logo}
              {name}
            </span>
          )}
        </li>
      ))}
    </ul>
  );
}

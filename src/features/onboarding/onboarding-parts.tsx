"use client";

import type { ReactNode } from "react";
import { ArrowLeft } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Panel } from "@/components/ui/panel";
import { StatusDot } from "@/components/ui/status-pill";
import { TypeIcon } from "@/components/ui/type-icon";
import { Icon, type IconName } from "@/components/ui/icon";
import { cn } from "@/lib/utils";
import { useT } from "@/lib/i18n";

/** A step's centred title and description. */
export function StepHeader({ title, description }: { title: ReactNode; description: string }) {
  return (
    <div className="text-center">
      <h1 className="page-title">{title}</h1>
      <p className="mx-auto mt-3 max-w-sm text-[0.875rem] leading-relaxed text-muted-foreground">{description}</p>
    </div>
  );
}

/** One thing the app does, in the welcome's list. */
export function Feature({ icon, title, description }: { icon: IconName; title: string; description: string }) {
  return (
    <div className="flex items-start gap-3 border-t border-border px-4 py-3 first:border-t-0">
      <TypeIcon>
        <Icon name={icon} className="size-4" />
      </TypeIcon>
      <div className="min-w-0">
        <p className="text-[0.8125rem] font-medium text-foreground">{title}</p>
        <p className="mt-0.5 text-[0.75rem] leading-relaxed text-muted-foreground">{description}</p>
      </div>
    </div>
  );
}

/** One line of the final summary: done or not, and how. */
export function SummaryRow({ ok, label, value }: { ok: boolean; label: string; value: string }) {
  return (
    <Panel className="flex min-h-[3.25rem] items-center justify-between gap-4 px-4">
      <div className="flex items-center gap-2.5">
        <StatusDot tone={ok ? "ok" : "muted"} />
        <span className="text-[0.8125rem] font-medium text-foreground">{label}</span>
      </div>
      <span className={cn("text-[0.75rem]", ok ? "text-foreground" : "text-muted-foreground")}>{value}</span>
    </Panel>
  );
}

/** A step's footer: pinned to the bottom of the onboarding's scroller, so its buttons stay in
 *  view however tall the step is (and in the same place from step to step); the content scrolls
 *  under it, behind a short fade. */
export function StepFooter({ className, children }: { className?: string; children: ReactNode }) {
  return (
    <div
      data-step-footer
      className={cn(
        "sticky bottom-0 z-10 -mx-3 mt-auto bg-background px-3 pt-6 pb-6 before:pointer-events-none before:absolute before:inset-x-0 before:-top-6 before:h-6 before:bg-gradient-to-t before:from-background before:to-transparent before:content-['']",
        className,
      )}
    >
      {children}
    </div>
  );
}

/** Back (ghost) on the left, the step's single primary action on the right. */
export function StepActions({
  onBack,
  onNext,
  nextLabel,
  disabled,
  backDisabled,
}: {
  onBack: () => void;
  onNext: () => void;
  nextLabel: ReactNode;
  disabled?: boolean;
  backDisabled?: boolean;
}) {
  const t = useT();
  return (
    <StepFooter className="flex items-center justify-between gap-2">
      <Button variant="ghost" onClick={onBack} disabled={backDisabled}>
        <ArrowLeft className="size-4" /> {t("onboarding.back")}
      </Button>
      <Button onClick={onNext} disabled={disabled}>
        {nextLabel}
      </Button>
    </StepFooter>
  );
}

/** Where you are: one dot per step (the current one wide), then a mono "3 / 9 · Step name". */
export function StepProgress({
  count,
  pos,
  icon: StepIcon,
  name,
}: {
  count: number;
  pos: number;
  icon?: React.ComponentType<{ className?: string }>;
  name: string;
}) {
  return (
    <>
      <div className="mt-6 flex items-center gap-1.5" aria-hidden>
        {Array.from({ length: count }, (_, n) => (
          <span
            key={n}
            className={cn(
              "h-1.5 rounded-full transition-all duration-300",
              n === pos ? "w-4 bg-jewel-solid" : n < pos ? "w-1.5 bg-muted-foreground" : "w-1.5 bg-border-strong",
            )}
          />
        ))}
      </div>
      <p className="mt-2.5 flex items-center gap-1.5 font-mono text-[0.6875rem] text-faint">
        {StepIcon && <StepIcon className="size-3" />}
        {pos + 1} / {count} · {name}
      </p>
    </>
  );
}

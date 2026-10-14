"use client";

import { useRef, useState, useSyncExternalStore } from "react";
import { AlertTriangle, Plug } from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { RadioList, RadioRow } from "@/components/ui/radio-row";
import { Switch } from "@/components/ui/switch";
import { setPortMode, type PortMode } from "@/lib/settings";
import { useFocusTrap } from "@/lib/use-focus-trap";
import { useT } from "@/lib/i18n";

// Before a container lab starts on this machine: random host ports, or the lab's own ones.
// `askPortMode` opens the dialog (mounted once, in the app shell) and resolves with the choice,
// or null when the player cancels.

type Ask = { id: number; title: string; resolve: (mode: PortMode | null) => void };

let current: Ask | null = null;
let asked = 0;
const listeners = new Set<() => void>();
const emit = () => listeners.forEach((l) => l());

export function askPortMode(title: string): Promise<PortMode | null> {
  // A second start while the dialog is open answers the first one with a cancel.
  current?.resolve(null);
  return new Promise((resolve) => {
    current = { id: ++asked, title, resolve };
    emit();
  });
}

function answer(mode: PortMode | null) {
  const ask = current;
  current = null;
  emit();
  ask?.resolve(mode);
}

const subscribe = (l: () => void) => {
  listeners.add(l);
  return () => listeners.delete(l);
};

export function PortModePrompt() {
  const ask = useSyncExternalStore(
    subscribe,
    () => current,
    () => null,
  );
  // Keyed per request so each one starts on the recommended choice.
  return ask ? <Dialog key={ask.id} title={ask.title} /> : null;
}

function Dialog({ title }: { title: string }) {
  const t = useT();
  const [mode, setMode] = useState<PortMode>("random");
  const [remember, setRemember] = useState(false);
  const ref = useRef<HTMLDivElement>(null);
  // Escape answers "no start".
  useFocusTrap(ref, true, () => answer(null));

  const start = () => {
    if (remember) setPortMode(mode);
    answer(mode);
  };

  return (
    <div
      className="animate-fade-in fixed inset-0 z-50 flex items-center justify-center bg-[var(--scrim)] p-4 backdrop-blur-[2px]"
      onMouseDown={(e) => e.target === e.currentTarget && answer(null)}
    >
      <div
        ref={ref}
        tabIndex={-1}
        role="dialog"
        aria-modal="true"
        aria-labelledby="ports-title"
        className="surface-glass w-full max-w-[30rem] rounded-[1rem] p-5 outline-none"
      >
        <div className="flex items-start gap-3">
          <span className="mt-0.5 grid size-8 shrink-0 place-items-center rounded-control bg-jewel/10 text-jewel-text shadow-[inset_0_0_0_1px_color-mix(in_oklab,var(--jewel)_30%,transparent)]">
            <Plug className="size-4" />
          </span>
          <div className="min-w-0">
            <p id="ports-title" className="text-[0.9375rem] font-medium text-foreground">
              {t("labs.ports.question", { title })}
            </p>
            <p className="mt-1 text-[0.8125rem] leading-relaxed text-muted-foreground">{t("labs.ports.where")}</p>
          </div>
        </div>

        <PortChoice value={mode} onChange={setMode} className="mt-4" />

        <label className="mt-4 flex cursor-pointer items-center justify-between gap-3 text-[0.8125rem] text-muted-foreground">
          <span>
            {t("labs.ports.remember")} <span className="text-faint">{t("labs.ports.rememberHint")}</span>
          </span>
          <Switch aria-label={t("labs.ports.remember")} checked={remember} onCheckedChange={setRemember} />
        </label>

        <div className="mt-5 flex justify-end gap-2">
          <Button variant="ghost" size="sm" onClick={() => answer(null)}>
            {t("labs.ports.cancel")}
          </Button>
          <Button size="sm" onClick={start}>
            {t("labs.ports.startLab")}
          </Button>
        </div>
      </div>
    </div>
  );
}

/** Random free ports, or the lab's own ones with the collision warning. Also in "Where should it run?". */
export function PortChoice({ value, onChange, className }: { value: PortMode; onChange: (mode: PortMode) => void; className?: string }) {
  const t = useT();
  return (
    <RadioList label={t("labs.ports.hostPorts")} className={className}>
      <RadioRow
        selected={value === "random"}
        onSelect={() => onChange("random")}
        title={
          <>
            {t("labs.ports.random")} <Badge>{t("labs.ports.recommended")}</Badge>
          </>
        }
        subtitle={t("labs.ports.randomHint")}
      />
      <RadioRow selected={value === "default"} onSelect={() => onChange("default")} title={t("labs.ports.default")} subtitle={t("labs.ports.defaultHint")}>
        {value === "default" && (
          <p className="mt-2 flex items-start gap-2 rounded-control bg-warning/10 px-2.5 py-2 text-[0.75rem] leading-relaxed text-warning shadow-[inset_0_0_0_1px_color-mix(in_oklab,var(--warning)_30%,transparent)]">
            <AlertTriangle className="mt-0.5 size-3.5 shrink-0" />
            <span>{t("labs.ports.defaultWarning")}</span>
          </p>
        )}
      </RadioRow>
    </RadioList>
  );
}

"use client";

import { useRef, useState } from "react";
import { AlertTriangle } from "lucide-react";
import { Button } from "@/components/ui/button";
import { forceQuit, lingerQuit } from "@/lib/tauri";
import { useFocusTrap } from "@/lib/use-focus-trap";
import { useTauriEvent } from "@/lib/use-tauri-event";
import { useT } from "@/lib/i18n";

/**
 * When the user tries to quit while a lab is still deploying, the Rust side holds the window
 * open and emits `quit-blocked`; this shows a confirmation so an in-flight deploy (a cloud
 * apply especially, which keeps billing if cut off) isn't interrupted by accident.
 */
export function QuitGuard() {
  const t = useT();
  const [count, setCount] = useState<number | null>(null);
  const [quitting, setQuitting] = useState(false);
  const ref = useRef<HTMLDivElement>(null);

  useTauriEvent<number>("quit-blocked", (n) => setCount(typeof n === "number" ? n : 1));
  // Escape is "keep waiting", the choice that changes nothing.
  useFocusTrap(ref, count !== null, () => setCount(null));

  if (count === null) return null;

  return (
    <div className="animate-fade-in fixed inset-0 z-[60] flex items-center justify-center bg-[var(--scrim)] p-4 backdrop-blur-[0.125rem]">
      <div
        ref={ref}
        tabIndex={-1}
        role="alertdialog"
        aria-modal="true"
        aria-labelledby="quit-guard-title"
        className="w-full max-w-[26rem] surface-glass rounded-[1rem] p-5 outline-none"
      >
        <div className="flex items-start gap-3">
          <span className="mt-0.5 grid size-8 shrink-0 place-items-center rounded-control bg-warning/10 text-warning shadow-[inset_0_0_0_1px_color-mix(in_oklab,var(--warning)_30%,transparent)]">
            <AlertTriangle className="size-4" />
          </span>
          <div className="min-w-0">
            <p id="quit-guard-title" className="text-[0.9375rem] font-medium text-foreground">
              {t("app.quitGuard.title")}
            </p>
            <p className="mt-1 text-[0.8125rem] leading-relaxed text-muted-foreground">{t("app.quitGuard.body", { count })}</p>
          </div>
        </div>
        <div className="mt-5 flex justify-end gap-2">
          <Button variant="ghost" size="sm" onClick={() => setCount(null)}>
            {t("app.quitGuard.keepWaiting")}
          </Button>
          <Button
            variant="destructive"
            size="sm"
            disabled={quitting}
            onClick={() => {
              setQuitting(true);
              void forceQuit().catch(() => setQuitting(false));
            }}
          >
            {t("app.quitGuard.quitAnyway")}
          </Button>
          <Button
            size="sm"
            disabled={quitting}
            onClick={() => {
              setQuitting(true);
              void lingerQuit().catch(() => setQuitting(false));
            }}
          >
            {t("app.quitGuard.finishInBackground")}
          </Button>
        </div>
      </div>
    </div>
  );
}

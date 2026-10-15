"use client";

import { useEffect, useState } from "react";
import { X } from "lucide-react";
import { StatusDot, type Tone } from "@/components/ui/status-pill";
import { useT } from "@/lib/i18n";

/**
 * In-app toasts: glass cards at the bottom right, gone after a few seconds. `toast()` can be
 * called from anywhere (lib/failure's `tell` uses it while the window has focus; an OS
 * notification covers the rest).
 */
type Toast = { id: number; title: string; body?: string; tone: Tone };

const EVENT = "cyberctf:toast";
let seq = 0;

export function toast(title: string, body?: string, tone: Tone = "ok") {
  if (typeof window === "undefined") return;
  window.dispatchEvent(new CustomEvent<Toast>(EVENT, { detail: { id: ++seq, title, body, tone } }));
}

export function Toaster() {
  const t = useT();
  const [items, setItems] = useState<Toast[]>([]);
  useEffect(() => {
    const on = (e: Event) => {
      const item = (e as CustomEvent<Toast>).detail;
      setItems((list) => [...list.slice(-2), item]);
      setTimeout(
        () => {
          setItems((list) => list.filter((x) => x.id !== item.id));
        },
        item.tone === "fail" ? 7000 : 4000,
      );
    };
    window.addEventListener(EVENT, on);
    return () => {
      window.removeEventListener(EVENT, on);
    };
  }, []);
  // The live region stays mounted (empty when idle) so assistive tech is already watching it
  // when a toast arrives. It sits under modals (z-50): a toast never covers a dialog's buttons.
  return (
    <div aria-live="polite" className="pointer-events-none fixed right-5 bottom-5 z-40 flex w-[22rem] max-w-[calc(100vw-2.5rem)] flex-col gap-2">
      {items.map((item) => (
        <div
          key={item.id}
          // A failure interrupts (alert); anything else waits its turn (status).
          role={item.tone === "fail" ? "alert" : "status"}
          className="surface-glass animate-rise-in pointer-events-auto flex items-start gap-3 rounded-panel px-4 py-3 text-[0.8125rem]"
        >
          <StatusDot tone={item.tone} className="mt-1.5" />
          <div className="min-w-0 flex-1">
            <p className="font-medium text-foreground">{item.title}</p>
            {item.body && <p className="mt-0.5 line-clamp-3 break-words text-muted-foreground">{item.body}</p>}
          </div>
          <button
            type="button"
            aria-label={t("ui.toast.dismiss")}
            onClick={() => {
              setItems((list) => list.filter((x) => x.id !== item.id));
            }}
            className="-mr-1 grid size-6 place-items-center rounded-xs text-faint transition-colors hover:text-foreground"
          >
            <X className="size-3.5" />
          </button>
        </div>
      ))}
    </div>
  );
}

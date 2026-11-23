// The in-app toasts' sink: what code anywhere calls (lib/failure's `tell` while the window has
// focus, a sign-in, an update) to show one. The `Toaster` component, mounted once per window,
// listens and draws them; without one mounted (a test, a page without it) a toast goes nowhere.

/** How a toast reads: done, a warning, a failure, or a plain note. */
export type ToastTone = "ok" | "warn" | "fail" | "muted";

/** One toast, as the Toaster receives it. */
export type Toast = { id: number; title: string; body?: string; tone: ToastTone };

/** The window event a toast travels on. */
export const TOAST_EVENT = "cyberctf:toast";

let seq = 0;

/** Shows a toast in this window: `title`, with `body` under it. */
export function toast(title: string, body?: string, tone: ToastTone = "ok") {
  if (typeof window === "undefined") return;
  window.dispatchEvent(new CustomEvent<Toast>(TOAST_EVENT, { detail: { id: ++seq, title, body, tone } }));
}

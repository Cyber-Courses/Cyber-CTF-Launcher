"use client";

import { useEffect, useState, type ReactNode } from "react";
import { Panel } from "@/components/ui/panel";

// Development only. Inside the app's window a page renders as is. In a plain browser it has no
// Rust core to call: the app's own dev server (`pnpm dev`, the one `pnpm tauri dev` loads) shows
// a notice instead, and only the browser preview (`pnpm dev:web`, built with NEXT_PUBLIC_DEV_MOCK)
// answers the Tauri commands with the dev mock (src/lib/dev-mock/) when the URL has `?mock`. The
// mock is imported dynamically behind both build-time constants, so neither a production build
// nor `pnpm dev` holds it or its data.

const DEV = process.env.NODE_ENV !== "production";

let installing: Promise<void> | undefined;
/** Loads and installs the mock once per page (React's dev double effects call it twice); absent
 *  outside the browser preview, where neither it nor the import exists. */
const installMock =
  DEV && process.env.NEXT_PUBLIC_DEV_MOCK === "1"
    ? () =>
        (installing ??= import("@/lib/dev-mock").then((m) => {
          m.installDevMock();
        }))
    : undefined;

type Where = "app" | "mock" | "browser";

/** Where this page runs: in the app's window, in the browser preview with `?mock`, or in a plain
 *  browser. Tauri's internals in a page that says it is the mock's (`__CYBERCTF_MOCK__`: the mock
 *  itself, or a web end-to-end's stand-in it installs over) are not the app's. */
function where(): Where {
  if ("__TAURI_INTERNALS__" in window && !(window as { __CYBERCTF_MOCK__?: boolean }).__CYBERCTF_MOCK__) return "app";
  return installMock && new URLSearchParams(window.location.search).has("mock") ? "mock" : "browser";
}

/** What a plain browser gets from a dev server instead of the app, which would fail on its first
 *  Tauri call. */
function BrowserNotice() {
  const cmd = "rounded bg-muted px-1 font-mono text-[0.75rem] text-foreground";
  return (
    <main className="flex min-h-screen items-center justify-center p-6">
      <Panel className="max-w-md">
        <div className="space-y-2 px-5 py-4 text-[0.8125rem] text-muted-foreground">
          {installMock ? (
            <p>
              This is the browser preview of the Cyber CTF desktop app. Open it with{" "}
              <a className="text-link underline" href="?mock">
                ?mock
              </a>{" "}
              in the address to run it on sample data.
            </p>
          ) : (
            <p>
              This is the Cyber CTF desktop app&apos;s interface. It only runs inside the app (<code className={cmd}>pnpm tauri dev</code>). For the browser
              preview with sample data, run <code className={cmd}>pnpm dev:web</code>.
            </p>
          )}
        </div>
      </Panel>
    </main>
  );
}

/** In development: its children once the page is in the app's window or the mock is installed
 *  (so their first Tauri call already goes to it), else the browser notice. */
function DevGate({ children }: { children: ReactNode }) {
  const [shown, setShown] = useState<"children" | "notice" | null>(null);
  useEffect(() => {
    const here = where();
    // Settled in a callback in every case, so the effect itself sets no state.
    const settled = here === "mock" && installMock ? installMock() : Promise.resolve();
    let live = true;
    settled.then(
      () => {
        if (live) setShown(here === "browser" ? "notice" : "children");
      },
      (e: unknown) => {
        console.error("dev mock failed to load", e);
        if (live) setShown("children");
      },
    );
    return () => {
      live = false;
    };
  }, []);
  if (shown === "notice") return <BrowserNotice />;
  return shown === "children" ? children : null;
}

/** Production: a plain pass-through (there is no server for a browser to reach). */
function PassThrough({ children }: { children: ReactNode }) {
  return children;
}

/** Wraps every page: see the comment above. A production build keeps only the pass-through. */
export const DevMockGate = DEV ? DevGate : PassThrough;

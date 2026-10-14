"use client";

import { useEffect, useState, type ReactNode } from "react";

// Development only: ?mock in a plain browser answers the Tauri commands with sample data
// (src/lib/dev-mock.ts). It is imported dynamically behind the NODE_ENV check, so a production
// build leaves the mock and its data out of the bundle altogether.

let installing: Promise<void> | undefined;
/** Loads and installs the mock once per page (React's dev double effects call it twice). The
 *  branch is decided at build time: in production neither it nor the import exists. */
const installMock =
  process.env.NODE_ENV === "production" ? () => Promise.resolve() : () => (installing ??= import("@/lib/dev-mock").then((m) => m.installDevMock()));

/** Renders its children; in development, only once the mock is installed (when `?mock` asks for
 *  it), so their first Tauri call already goes to it. In production it is a plain pass-through. */
export function DevMockGate({ children }: { children: ReactNode }) {
  const [ready, setReady] = useState(process.env.NODE_ENV === "production");
  useEffect(() => {
    if (process.env.NODE_ENV === "production") return;
    let live = true;
    installMock().then(
      () => live && setReady(true),
      (e) => {
        console.error("dev mock failed to load", e);
        if (live) setReady(true);
      },
    );
    return () => {
      live = false;
    };
  }, []);
  return ready ? children : null;
}

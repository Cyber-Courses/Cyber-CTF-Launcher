import type { NextConfig } from "next";
import { PHASE_DEVELOPMENT_SERVER } from "next/constants";

// Tauri serves the frontend as static files: no Next server, API routes,
// middleware or SSR. Anything privileged goes through Rust commands (src-tauri).
export default function nextConfig(phase: string): NextConfig {
  // The browser preview (`pnpm dev:web`, scripts/dev/web.ts): a dev server whose pages answer the
  // Tauri commands with the dev mock. Never a build, nor the app's own dev server (`pnpm dev`).
  const browserPreview = phase === PHASE_DEVELOPMENT_SERVER && process.env.NEXT_PUBLIC_DEV_MOCK === "1";
  return {
    output: "export",
    images: { unoptimized: true },
    // Inlined in every build, set or not, so a release build without them is a constant `""` there
    // and leaves the GUI e2e's hooks (src/components/wdio-hooks.tsx) and the dev mock
    // (src/components/dev-mock-gate.tsx) out altogether.
    env: { NEXT_PUBLIC_WDIO: process.env.NEXT_PUBLIC_WDIO ?? "", NEXT_PUBLIC_DEV_MOCK: browserPreview ? "1" : "" },
    // Its own folder (and so its own dev lock), so the preview runs beside `pnpm tauri dev`.
    distDir: browserPreview ? ".next-web" : ".next",
    // Next's dev server already refuses its dev endpoints and bundles to any origin but `localhost`
    // and the host it listens on (127.0.0.1, package.json's scripts): no other origin is added.
    allowedDevOrigins: [],
  };
}

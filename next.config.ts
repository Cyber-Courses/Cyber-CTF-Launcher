import type { NextConfig } from "next";

// Tauri serves the frontend as static files: no Next server, API routes,
// middleware or SSR. Anything privileged goes through Rust commands (src-tauri).
const nextConfig: NextConfig = {
  output: "export",
  images: { unoptimized: true },
  // Inlined in every build, set or not, so a release build without it is a constant `""` there and
  // leaves the GUI e2e's hooks out altogether (src/components/wdio-hooks.tsx).
  env: { NEXT_PUBLIC_WDIO: process.env.NEXT_PUBLIC_WDIO ?? "" },
};

export default nextConfig;

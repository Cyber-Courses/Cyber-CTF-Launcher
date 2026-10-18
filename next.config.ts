import type { NextConfig } from "next";

// Tauri serves the frontend as static files: no Next server, API routes,
// middleware or SSR. Anything privileged goes through Rust commands (src-tauri).
const nextConfig: NextConfig = {
  output: "export",
  images: { unoptimized: true },
};

export default nextConfig;

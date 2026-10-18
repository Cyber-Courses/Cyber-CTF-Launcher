import { useId } from "react";
import { cn } from "@/lib/utils";

// Cyber CTF's mark: the layered up-chevron (cyber-design-system/marks/paths.js). Two flat layers
// with a gradient where they overlap; the colours are tokens (--mark-top, --mark-base), so the
// base darkens in Light mode.
const CTF_TOP =
  "M164.97,17.98L7.02,163.02c-6.36,5.84-8.65,14.89-5.84,23.05l19.62,56.95c4.68,13.59,21.05,18.96,32.87,10.77l133.3-92.3c14.54-10.07,33.81-10.07,48.35,0l133.3,92.3c11.82,8.18,28.19,2.82,32.87-10.77l19.62-56.95c2.81-8.16.52-17.21-5.84-23.05L257.31,17.98c-26.11-23.97-66.23-23.97-92.34,0Z";
const CTF_BASE =
  "M164.97,82.45L7.02,227.48c-6.36,5.84-8.65,14.89-5.84,23.05l19.62,56.95c4.68,13.59,21.05,18.96,32.87,10.77l133.3-92.3c14.54-10.07,33.81-10.07,48.35,0l133.3,92.3c11.82,8.18,28.19,2.82,32.87-10.77l19.62-56.95c2.81-8.16.52-17.21-5.84-23.05l-157.95-145.03c-26.11-23.97-66.23-23.97-92.34,0Z";

export function CtfMark({ className }: { className?: string }) {
  const id = useId().replace(/:/g, "");
  return (
    <svg viewBox="-6 -40 434.29 400" aria-hidden className={cn("size-5 shrink-0", className)}>
      <defs>
        <clipPath id={`c${id}`}>
          <path d={CTF_BASE} />
        </clipPath>
        <linearGradient id={`g${id}`} x1="0" y1="0" x2="0" y2="257.66" gradientUnits="userSpaceOnUse">
          <stop offset=".36" style={{ stopColor: "var(--mark-base)" }} />
          <stop offset="1" style={{ stopColor: "var(--mark-top)" }} />
        </linearGradient>
      </defs>
      <path d={CTF_BASE} style={{ fill: "var(--mark-base)" }} />
      <path d={CTF_TOP} style={{ fill: "var(--mark-top)" }} />
      <path d={CTF_TOP} fill={`url(#g${id})`} clipPath={`url(#c${id})`} />
    </svg>
  );
}

import { Server } from "lucide-react";
import type { Provider } from "@/lib/tauri";

/** Brand marks we ship for local hypervisors (Simple Icons, CC0; marks belong to their owners).
 *  No VMware mark: it needs Broadcom's approval. Others fall back to a neutral server icon. */
const LOGOS: Partial<Record<Provider, string>> = {
  virtualbox: "/brands/virtualbox.svg",
  qemu: "/brands/qemu.svg",
};

/** A hypervisor's logo on a white logo tile (white in every mode) (like the container engines), or a neutral icon. */
export function HypervisorLogo({ provider, size = "md" }: { provider: Provider | null | undefined; size?: "sm" | "md" }) {
  const logo = provider ? LOGOS[provider] : undefined;
  const box = size === "sm" ? "size-6 rounded-sm p-1" : "size-8 rounded-control p-1.5";
  return logo ? (
    <span className={`logo-tile flex shrink-0 items-center justify-center ${box}`}>
      {/* eslint-disable-next-line @next/next/no-img-element -- static export, plain asset */}
      <img src={logo} alt="" className="size-full object-contain" />
    </span>
  ) : (
    <span
      className={`flex shrink-0 items-center justify-center bg-glass-2 text-muted-foreground shadow-[inset_0_0_0_1px_var(--input)] ${box.replace(/p-[\d.]+/, "")}`}
    >
      <Server className={size === "sm" ? "size-3.5" : "size-4"} />
    </span>
  );
}

import { Server } from "lucide-react";
import type { Provider } from "@/lib/tauri";

/** Brand marks we ship for local hypervisors (marks belong to their owners). VirtualBox and QEMU
 *  are Simple Icons (CC0). UTM's is its own app icon, from its repository (utmapp/UTM at commit
 *  12740342, `Platform/Assets.xcassets/AppIcon.appiconset/icon_128pt.png`), Apache-2.0, shipped
 *  with that license beside it (`/brands/utm.LICENSE`). No VMware mark: it needs Broadcom's
 *  approval. Parallels and Hyper-V are commercial marks with their own rules: they, and anything
 *  else, fall back to a neutral server icon. */
const LOGOS: Partial<Record<Provider, string>> = {
  virtualbox: "/brands/virtualbox.svg",
  qemu: "/brands/qemu.svg",
  utm: "/brands/utm.png",
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

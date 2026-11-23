import { LogoTile } from "@/components/ui/logo-tile";
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

/** A hypervisor's mark (its logo shipped above, or none). */
export const hypervisorLogo = (provider: Provider | null | undefined): string | undefined => (provider ? LOGOS[provider] : undefined);

/** A hypervisor's logo on a white logo tile (white in every mode) (like the container engines), or a neutral icon. */
export function HypervisorLogo({ provider, size = "md" }: { provider: Provider | null | undefined; size?: "sm" | "md" }) {
  return <LogoTile src={hypervisorLogo(provider)} size={size} />;
}

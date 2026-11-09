import { Server } from "lucide-react";
import { CLOUD_PROVIDERS, PROVIDER_NAMES, type RemoteProvider } from "@/lib/tauri";
import { TypeIcon } from "@/components/ui/type-icon";
import { cloudLogo } from "@/features/servers/host-setup/constants";

/** Brand logos we can show for a host/hypervisor. VMware/Broadcom's logo needs their approval,
 *  and the local hypervisors (VirtualBox, Hyper-V, Parallels, QEMU, UTM) have no bundled mark
 *  yet, so those fall back to a neutral glyph. Drop an SVG in public/brands and add it here to
 *  light one up. Proxmox uses its icon mark (not the wide lockup) so it reads in the tile. */
const LOGO: Partial<Record<RemoteProvider, { src: string; alt: string }>> = {
  proxmox: { src: "/brands/proxmox-icon.svg", alt: "Proxmox" },
  ...Object.fromEntries(CLOUD_PROVIDERS.map((id) => [id, { src: cloudLogo(id), alt: PROVIDER_NAMES[id] }])),
};

/** The leading mark for a host row: the provider's logo in a tile when we have one, else a
 *  neutral badge in the same tile, so every row's glyph lines up. */
export function ProviderGlyph({ provider }: { provider: RemoteProvider }) {
  const logo = LOGO[provider];
  return (
    <TypeIcon>
      {logo ? (
        // eslint-disable-next-line @next/next/no-img-element -- static export, plain asset
        <img src={logo.src} alt={logo.alt} className="size-5 object-contain" draggable={false} />
      ) : (
        <Server className="size-4" />
      )}
    </TypeIcon>
  );
}

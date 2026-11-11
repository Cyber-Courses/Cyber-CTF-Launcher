import { Server } from "lucide-react";

const BOX = { sm: "size-6 rounded-sm", md: "size-8 rounded-control" } as const;
const PAD = { sm: "p-1", md: "p-1.5" } as const;

/**
 * A brand mark in a fixed square, the same for every provider the launcher names: the mark on a
 * white logo tile (white in every mode, so dark artwork stays legible), an app icon (`fill`)
 * filling the square itself, or, for a provider without a mark we may ship (VMware's needs
 * Broadcom's approval; Hyper-V's and Parallels' have rules of their own), a neutral server glyph
 * on glass. Decorative: the provider's name is always beside it.
 */
export function LogoTile({ src, fill = false, size = "md" }: { src?: string; fill?: boolean; size?: "sm" | "md" }) {
  if (!src)
    return (
      <span className={`flex shrink-0 items-center justify-center bg-glass-2 text-muted-foreground shadow-[inset_0_0_0_1px_var(--input)] ${BOX[size]}`}>
        <Server className={size === "sm" ? "size-3.5" : "size-4"} />
      </span>
    );
  // eslint-disable-next-line @next/next/no-img-element -- static export, plain asset
  const mark = <img src={src} alt="" className={fill ? `${BOX[size]} shrink-0 object-contain` : "size-full object-contain"} draggable={false} />;
  return fill ? mark : <span className={`logo-tile flex shrink-0 items-center justify-center ${BOX[size]} ${PAD[size]}`}>{mark}</span>;
}

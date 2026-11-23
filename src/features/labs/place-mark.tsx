"use client";

import { Laptop } from "lucide-react";
import { EngineMark } from "@/features/machine/engine-logo";
import { ENGINES } from "@/features/machine/setup-steps/engines";
import { HypervisorLogo } from "@/features/machine/hypervisor-logo";
import { ProviderGlyph } from "@/features/servers/provider-glyph";
import type { Place } from "@/features/labs/place";
import type { Provider } from "@/lib/tauri";

/** The logo of where a lab runs, for the network diagram's host card: the engine's, the
 *  hypervisor's, the server kind's or the cloud's; this machine's plain mark when none is known. */
export function PlaceMark({ place }: { place: Place | null }) {
  // An engine with a mark of its own (Podman has none: this machine's plain mark).
  if (place?.kind === "engine" && place.engine && ENGINES.some((e) => e.id === place.engine)) return <EngineMark id={place.engine} size="sm" />;
  if (place?.kind === "hypervisor" && place.provider) return <HypervisorLogo provider={place.provider as Provider} size="sm" />;
  if (place?.kind === "cloud") return <ProviderGlyph provider={place.provider} />;
  if (place?.kind === "server" && place.provider) return <ProviderGlyph provider={place.provider} />;
  return (
    <span className="hostcard-icon">
      <Laptop size={16} />
    </span>
  );
}

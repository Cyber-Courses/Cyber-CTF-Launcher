"use client";

import type { ReactNode } from "react";
import { useT, type T } from "@/lib/i18n";
import type { Machine } from "@/features/labs/network-diagram/model";
import { CopyText } from "@/features/labs/network-diagram/copy-text";
import { PlaceMark } from "@/features/labs/place-mark";
import { placeName, type Place } from "@/features/labs/place";
import { PROVIDER_NAMES } from "@/lib/tauri";

// Kept apart from the diagram (React Flow, ELK) so the lab page can draw the card's header while
// the diagram's code loads (see lab-detail/network-panel.tsx).

/** The card's name: the engine or hypervisor the lab runs on here, the server, or the cloud;
 *  "Your machine" when the place isn't known. */
function title(t: T, place: Place | null): string {
  if (!place) return t("diagram.host.yourMachine");
  return placeName(place) || t("diagram.host.yourMachine");
}

/** Under the name: this machine's loopback address for containers here, what the VMs are for
 *  a hypervisor, the server's kind and address, or the cloud's region and account. */
function Sub({ place }: { place: Place | null }) {
  const t = useT();
  if (!place || place.kind === "engine")
    return (
      <>
        <CopyText text="127.0.0.1">127.0.0.1</CopyText> · {t("diagram.host.host")}
      </>
    );
  if (place.kind === "hypervisor") return <>{place.inVm ? t("diagram.host.containerVmSub") : t("diagram.host.vmSub")}</>;
  if (place.kind === "server")
    return (
      <>
        {place.provider && place.address
          ? t("diagram.host.serverSubWith", { kind: PROVIDER_NAMES[place.provider], address: place.address })
          : t("diagram.host.serverSub")}
      </>
    );
  return <>{t("diagram.host.cloudSub", { region: place.region, account: place.name })}</>;
}

/** The card's header: where the lab runs, with its logo (the engine's, the hypervisor's, the
 *  server kind's or the cloud's), then the view's controls (`tools`, see fit-width.tsx) when the
 *  lab is wider than the card. */
export function HostHeader({ place = null, tools }: { place?: Place | null; tools?: ReactNode }) {
  const t = useT();
  return (
    <div className="hostcard-header">
      <PlaceMark place={place} />
      <div>
        <div className="hostcard-title">{title(t, place)}</div>
        <div className="hostcard-sub mono">
          <Sub place={place} />
        </div>
      </div>
      {tools}
      <span className="hostcard-online">
        <i /> {t("diagram.host.online")}
      </span>
    </div>
  );
}

/** Whether the card has published-port tabs on its bottom edge (they need room below it). */
export const hasPortTabs = (machines: Machine[]) => machines.some((m) => m.ports.some((p) => p.published > 0));

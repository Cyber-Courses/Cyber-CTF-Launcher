"use client";

import type { ReactNode } from "react";
import { Laptop, Server } from "lucide-react";
import { useT } from "@/lib/i18n";
import type { Machine } from "@/features/labs/network-diagram/model";
import { CopyText } from "@/features/labs/network-diagram/copy-text";

// Kept apart from the diagram (React Flow, ELK) so the lab page can draw the card's header while
// the diagram's code loads (see lab-detail/network-panel.tsx).

/** The card's header: your machine (127.0.0.1) or the server the lab runs on, then the view's
 *  controls (`tools`, see fit-width.tsx) when the lab is wider than the card. */
export function HostHeader({ host, tools }: { host: string | null; tools?: ReactNode }) {
  const t = useT();
  return (
    <div className="hostcard-header">
      <span className="hostcard-icon">{host ? <Server size={16} /> : <Laptop size={16} />}</span>
      <div>
        <div className="hostcard-title">{host ?? t("diagram.host.yourMachine")}</div>
        <div className="hostcard-sub mono">
          {host ? (
            t("diagram.host.serverSub")
          ) : (
            <>
              <CopyText text="127.0.0.1">127.0.0.1</CopyText> · {t("diagram.host.host")}
            </>
          )}
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

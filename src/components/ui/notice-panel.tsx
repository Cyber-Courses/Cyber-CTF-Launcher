import type { ReactNode } from "react";
import { Panel } from "@/components/ui/panel";
import { StatusDot } from "@/components/ui/status-pill";

/** A compact warning or error panel: a status dot, then the message in muted ink. */
export function NoticePanel({ tone, children }: { tone: "warn" | "fail"; children: ReactNode }) {
  return (
    <Panel>
      <div className="flex items-start gap-3 px-4 py-3 text-[0.8125rem]">
        <StatusDot tone={tone} className="mt-1.5" />
        <p className="min-w-0 break-words text-muted-foreground">{children}</p>
      </div>
    </Panel>
  );
}

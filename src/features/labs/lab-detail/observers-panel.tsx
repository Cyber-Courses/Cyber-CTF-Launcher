"use client";

import { Panel, PanelHeader } from "@/components/ui/panel";
import type { LabTool } from "@/lib/tauri";
import { useT } from "@/lib/i18n";

/** The lab's observers (`tools:` in its spec) and where each answers. */
export function ObserversPanel({ tools }: { tools: LabTool[] }) {
  const t = useT();
  return (
    <Panel>
      <PanelHeader title={t("labs.detail.observers")} meta={String(tools.length)} />
      <div>
        {tools.map((tool) => (
          <div key={tool.name} className="flex flex-wrap items-baseline gap-x-3 gap-y-0.5 border-t border-border px-4 py-2.5 text-[0.8125rem] first:border-t-0">
            <span className="font-medium text-foreground">{tool.name}</span>
            <span className="font-mono text-[0.6875rem] text-faint">
              {tool.addresses.map((a) => `${a.network} ${a.ip}`).join(" · ")}
              {tool.publish ? ` · http://127.0.0.1:${String(tool.publish)}` : ""}
            </span>
          </div>
        ))}
      </div>
    </Panel>
  );
}

"use client";

import { Play, RotateCcw } from "lucide-react";
import { Panel, PanelHeader } from "@/components/ui/panel";
import { Button } from "@/components/ui/button";
import { Spinner } from "@/components/ui/spinner";
import { TypeIcon } from "@/components/ui/type-icon";
import type { Lab } from "@/features/labs/use-labs";
import { formatAgo } from "@/lib/format";
import { useT } from "@/lib/i18n";

/** "Jump back in": the labs launched last, each one click from running again. */
export function JumpBackPanel({
  recent,
  now,
  canStart,
  isBusy,
  onOpen,
  onStart,
}: {
  recent: { lab: Lab; ts: number }[];
  now: number;
  canStart: (lab: Lab) => boolean;
  isBusy: (lab: Lab) => boolean;
  onOpen: (lab: Lab) => void;
  onStart: (lab: Lab) => void;
}) {
  const t = useT();
  return (
    <Panel>
      <PanelHeader title={t("home.jumpBack.title")} meta={t("home.jumpBack.meta")} />
      {recent.map(({ lab, ts }) => (
        <div
          key={lab.id}
          className="flex h-13 items-center gap-3.5 border-t border-border px-4 text-[0.8125rem] transition-colors first:border-t-0 hover:bg-glass"
        >
          <TypeIcon>
            <RotateCcw className="size-3.5" />
          </TypeIcon>
          <button
            onClick={() => {
              onOpen(lab);
            }}
            className="min-w-0 flex-1 text-left"
          >
            <span className="block truncate font-medium text-foreground">{lab.title}</span>
            <span className="block truncate font-mono text-[0.6875rem] text-faint">
              {t("home.jumpBack.lastRun", { slug: lab.slug, ago: formatAgo(ts, now) })}
            </span>
          </button>
          <Button
            size="xs"
            onClick={() => {
              onStart(lab);
            }}
            disabled={isBusy(lab) || !canStart(lab)}
          >
            {isBusy(lab) ? (
              <Spinner className="size-3" />
            ) : (
              <>
                <Play /> {t("home.jumpBack.resume")}
              </>
            )}
          </Button>
        </div>
      ))}
    </Panel>
  );
}

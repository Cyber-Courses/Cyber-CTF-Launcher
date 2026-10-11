"use client";

import { useEffect, useState } from "react";
import { Container, Server, Square } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Panel, PanelHeader } from "@/components/ui/panel";
import { Spinner } from "@/components/ui/spinner";
import { TypeIcon } from "@/components/ui/type-icon";
import { PROVIDER_LABELS } from "@/features/machine/hypervisors";
import { ListSkeleton } from "@/features/machine/machine-parts";
import { useMachineFormat } from "@/features/machine/use-machine-format";
import { useT } from "@/lib/i18n";
import { apiQuery, machineWorkloadStop, machineWorkloads, type Workload } from "@/lib/tauri";
import { ignore } from "@/lib/failure";
import { usePoll } from "@/lib/use-poll";

/** What's running on this machine (Docker and Vagrant), with a Stop per lab. Polled every 8s,
 *  and again whenever `refreshKey` changes (e.g. after a setup test). */
export function RunningNowPanel({ refreshKey }: { refreshKey: unknown }) {
  const t = useT();
  const fmt = useMachineFormat();
  const [workloads, setWorkloads] = useState<Workload[] | null>(null);
  const [stopping, setStopping] = useState<string | null>(null);
  // One workloads read at a time: the read shells out to docker/vagrant and can be slow, so the
  // 8s poll must not stack reads on top of one still in flight.
  const load = usePoll(machineWorkloads, 8000, { serial: true, restartKey: refreshKey, onValue: setWorkloads, onError: () => setWorkloads([]) });

  // Lab titles for the list (the runtime only knows ids).
  const [titles, setTitles] = useState<Record<string, string>>({});
  useEffect(() => {
    apiQuery<{ labs: { id: string; title: string }[] }>("labTitles")
      .then((d) => setTitles(Object.fromEntries(d.labs.map((l) => [l.id, l.title]))))
      .catch(ignore("lab ids are shown instead of titles"));
  }, []);

  async function stop(w: Workload) {
    setStopping(`${w.kind}:${w.id}`);
    try {
      await machineWorkloadStop(w.kind, w.id);
    } catch {
      /* the list shows what's still running */
    } finally {
      setStopping(null);
      load();
    }
  }

  const labMem = (workloads ?? []).reduce((a, w) => a + w.memBytes, 0);
  return (
    <Panel>
      <PanelHeader
        title={t("machine.running.title")}
        meta={labMem > 0 ? <span className="tabular-nums">{t("machine.running.inUse", { size: fmt.bytes(labMem) })}</span> : undefined}
      />
      {workloads === null ? (
        <ListSkeleton />
      ) : workloads.length === 0 ? (
        <p className="px-4 py-3.5 text-[0.8125rem] text-muted-foreground">{t("machine.running.empty")}</p>
      ) : (
        workloads.map((w) => {
          const key = `${w.kind}:${w.id}`;
          const name = w.id === "selftest" ? t("machine.running.setupTest") : (titles[w.id] ?? w.id);
          const meta =
            w.kind === "docker"
              ? `${t("machine.running.containers", { count: w.count })}${w.memBytes ? ` · ${fmt.bytes(w.memBytes)}` : ""}`
              : `${t("machine.running.vms", { count: w.count })}${w.provider ? ` · ${PROVIDER_LABELS[w.provider] ?? w.provider}` : ""}`;
          return (
            <div
              key={key}
              className="flex min-h-[3.25rem] items-center gap-3 border-t border-border px-4 py-2 transition-colors first:border-t-0 hover:bg-glass"
            >
              <TypeIcon>{w.kind === "docker" ? <Container className="size-4" /> : <Server className="size-4" />}</TypeIcon>
              <div className="min-w-0 flex-1">
                <p className="truncate text-[0.8125rem] font-medium text-foreground">{name}</p>
                <p className="truncate font-mono text-[0.6875rem] tabular-nums text-faint">{meta}</p>
              </div>
              <Button variant="outline" size="xs" onClick={() => stop(w)} disabled={stopping !== null}>
                {stopping === key ? (
                  <>
                    <Spinner className="size-3" /> {t("machine.running.stopping")}
                  </>
                ) : (
                  <>
                    <Square className="size-3" /> {t("machine.running.stop")}
                  </>
                )}
              </Button>
            </div>
          );
        })
      )}
    </Panel>
  );
}

"use client";

import { useCallback, useEffect, useRef, useState } from "react";
import { Plus, RefreshCw } from "lucide-react";
import { Panel, PanelHeader } from "@/components/ui/panel";
import { Button } from "@/components/ui/button";
import { NoticePanel } from "@/components/ui/notice-panel";
import { Skeleton } from "@/components/ui/skeleton";
import { Spinner } from "@/components/ui/spinner";
import {
  serverCapacity,
  serverOpenSetup,
  serverRemove,
  serverRunningLabs,
  serverSetDefault,
  systemCheck,
  type HostCapacity,
  type SystemReport,
} from "@/lib/tauri";
import { PageHeader } from "@/components/ui/page-header";
import { HostRow } from "@/features/servers/host-row";
import { CapacityPanel } from "@/features/servers/capacity-panel";
import { ServersEmptyState } from "@/features/servers/servers-empty-state";
import { StatusDot } from "@/components/ui/status-pill";
import { VmTest, loadVmTests, saveVmTest } from "@/features/servers/vm-tests";
import { canRunVmHere, capacityHost, hostsSummary, onPremHosts } from "@/features/servers/servers-model";
import { useHostTests, useServerList } from "@/features/servers/use-server-list";
import { ignore, warn } from "@/lib/failure";
import { useNow } from "@/lib/use-now";
import { usePolled } from "@/lib/use-poll";
import { useT } from "@/lib/i18n";

type Tab = "setup";

export function ServerScreen({ onNavigate }: { onNavigate: (tab: Tab) => void }) {
  const t = useT();
  const { tests, test } = useHostTests();
  const [vmTestId, setVmTestId] = useState<string | null>(null);
  const [vmResults, setVmResults] = useState<Record<string, VmTest>>(() => loadVmTests());
  const [caps, setCaps] = useState<Record<string, HostCapacity | null>>({});
  const [report, setReport] = useState<SystemReport | null>(null);
  // Relative "Xm ago" labels, refreshed on a slow tick.
  const [now] = useNow(30_000);

  const loadCapacity = useCallback((id: string) => {
    serverCapacity(id)
      .then((c) => setCaps((m) => ({ ...m, [id]: c })))
      .catch(ignore("capacity is shown only when it can be read"));
  }, []);

  // Running-labs-per-host, refreshed on a slow poll (labs start and stop from other screens).
  const [running, loadRunning] = usePolled(() => serverRunningLabs().then((r) => r ?? {}), 8000, {} as Record<string, number>, {
    onError: ignore("read again on the next server change"),
  });

  // Auto-test and probe capacity for every host as it loads, so the row is live without a
  // click. The `seen` guard keeps a host from re-running these on every load; a change saved in
  // the setup window re-probes them all.
  const seen = useRef<Set<string>>(new Set());
  const { list, error, setError, reload } = useServerList({
    onLoaded: (l) => {
      // Only the hosts this page lists: the cloud accounts are tested on the Cloud page.
      for (const h of onPremHosts(l.hosts)) {
        if (!seen.current.has(h.id)) {
          seen.current.add(h.id);
          void test(h.id);
          loadCapacity(h.id);
        }
      }
    },
    onChanged: () => {
      seen.current.clear();
      loadRunning();
    },
  });
  // Cloud accounts (AWS/Azure/GCP) live on the Cloud page; the Servers page is on-prem hosts only.
  const hosts = list ? onPremHosts(list.hosts) : null;
  const defaultId = list?.default ?? null;

  useEffect(() => {
    systemCheck().then(setReport).catch(warn("system check"));
  }, []);

  const open = (id: string | null) => serverOpenSetup(id, "server").catch((e) => setError(String(e)));
  const testAll = () => hosts?.forEach((h) => test(h.id));
  async function act(fn: () => Promise<unknown>) {
    try {
      await fn();
      reload();
    } catch (e) {
      setError(String(e));
    }
  }

  const hasHosts = !!hosts && hosts.length > 0;
  const { online, anyTesting, tone: summaryTone } = hostsSummary(hosts ?? [], tests);
  const summaryText = !hasHosts
    ? t("servers.screen.noServer")
    : anyTesting
      ? t("servers.screen.checking")
      : t("servers.screen.onlineCount", { online, total: hosts.length });
  const capacity = capacityHost(hosts ?? [], defaultId, caps);

  return (
    <div className="space-y-5">
      <PageHeader
        title={t("servers.screen.title")}
        lead={t("servers.screen.lead")}
        actions={
          <>
            {hasHosts && (
              <Button variant="outline" size="sm" onClick={testAll} disabled={anyTesting}>
                {anyTesting ? <Spinner className="size-3.5" /> : <RefreshCw className="size-3.5" />} {t("servers.screen.testAll")}
              </Button>
            )}
            <Button variant="outline" size="sm" onClick={() => open(null)}>
              <Plus className="size-3.5" /> {t("servers.screen.addServer")}
            </Button>
          </>
        }
      />

      {error && <NoticePanel tone="fail">{error}</NoticePanel>}

      {hosts === null ? (
        <Panel>
          <div className="space-y-2 p-4">
            <Skeleton className="h-4 w-2/3" />
            <Skeleton className="h-4 w-1/2" />
          </div>
        </Panel>
      ) : !hasHosts ? (
        <ServersEmptyState onAdd={() => open(null)} />
      ) : (
        <Panel>
          <PanelHeader
            title={
              <>
                <StatusDot tone={summaryTone} pulse={anyTesting} />
                {t("servers.screen.hosts")}
              </>
            }
            meta={`${summaryText} · ${t("servers.screen.hostCount", { count: hosts.length })}`}
          />
          {hosts.map((h) => (
            <HostRow
              key={h.id}
              host={h}
              isDefault={h.id === defaultId}
              running={running[h.id] ?? 0}
              capacity={caps[h.id]}
              test={tests[h.id]}
              lastVm={vmResults[h.id]}
              now={now}
              vmTesting={vmTestId === h.id}
              onTest={() => test(h.id)}
              onVmTest={() => setVmTestId(vmTestId === h.id ? null : h.id)}
              onVmTestDone={(result) => {
                setVmResults(saveVmTest(h.id, result));
                void test(h.id);
                loadRunning();
              }}
              onEdit={() => open(h.id)}
              onDefault={() => act(() => serverSetDefault(h.id === defaultId ? null : h.id))}
              onRemove={() => {
                if (confirm(t("servers.screen.removeConfirm", { name: h.name }))) act(() => serverRemove(h.id));
              }}
            />
          ))}
        </Panel>
      )}

      {capacity && <CapacityPanel host={capacity.host} cap={capacity.cap} />}

      {/* Only worth suggesting when this machine can't already run VM labs itself. */}
      {report && !canRunVmHere(report) && (
        <Panel>
          <div className="flex flex-wrap items-center gap-3 px-4 py-3">
            <StatusDot tone="muted" />
            <p className="min-w-0 flex-1 text-[0.8125rem] text-muted-foreground">{t("servers.screen.noServerHint")}</p>
            <Button variant="ghost" size="xs" onClick={() => onNavigate("setup")}>
              {t("servers.screen.setUpMachine")}
            </Button>
          </div>
        </Panel>
      )}
    </div>
  );
}

"use client";

import { useState, type ReactNode } from "react";
import { Plus, RefreshCw } from "lucide-react";
import { Panel, PanelHeader } from "@/components/ui/panel";
import { Button } from "@/components/ui/button";
import { NoticePanel } from "@/components/ui/notice-panel";
import { Skeleton } from "@/components/ui/skeleton";
import { Spinner } from "@/components/ui/spinner";
import { serverRemove, serverSetDefault } from "@/lib/tauri";
import { PageHeader } from "@/components/ui/page-header";
import { HostRow } from "@/features/servers/host-row";
import { CapacityPanel } from "@/features/servers/capacity-panel";
import { ServersEmptyState } from "@/features/servers/servers-empty-state";
import { StatusDot, type Tone } from "@/components/ui/status-pill";
import { VmTest, loadVmTests, saveVmTest } from "@/features/servers/vm-tests";
import { canRunVmHere, capacityHost, hostsSummary } from "@/features/servers/servers-model";
import { useServerHosts } from "@/features/servers/use-server-hosts";
import { useNow } from "@/lib/use-now";
import { useT } from "@/lib/i18n";

type Tab = "setup";

export function ServerScreen({ onNavigate }: { onNavigate: (tab: Tab) => void }) {
  const t = useT();
  const { hosts, defaultId, tests, test, caps, running, loadRunning, report, error, open, testAll, act } = useServerHosts();
  const [vmTestId, setVmTestId] = useState<string | null>(null);
  const [vmResults, setVmResults] = useState<Record<string, VmTest>>(() => loadVmTests());
  // Relative "Xm ago" labels, refreshed on a slow tick.
  const [now] = useNow(30_000);

  const hasHosts = !!hosts && hosts.length > 0;
  const { online, anyTesting, tone } = hostsSummary(hosts ?? [], tests);
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
        <HostsLoading />
      ) : !hasHosts ? (
        <ServersEmptyState onAdd={() => open(null)} />
      ) : (
        <HostsPanel count={hosts.length} online={online} testing={anyTesting} tone={tone}>
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
                if (confirm(t("servers.screen.removeConfirm", { name: h.name }))) void act(() => serverRemove(h.id));
              }}
            />
          ))}
        </HostsPanel>
      )}

      {capacity && <CapacityPanel host={capacity.host} cap={capacity.cap} />}

      {/* Only worth suggesting when this machine can't already run VM labs itself. */}
      {report && !canRunVmHere(report) && <SetUpMachineHint onSetUp={() => onNavigate("setup")} />}
    </div>
  );
}

function HostsLoading() {
  return (
    <Panel>
      <div className="space-y-2 p-4">
        <Skeleton className="h-4 w-2/3" />
        <Skeleton className="h-4 w-1/2" />
      </div>
    </Panel>
  );
}

/** The hosts panel: a header summing up the hosts' last tests, then their rows. */
function HostsPanel({ count, online, testing, tone, children }: { count: number; online: number; testing: boolean; tone: Tone; children: ReactNode }) {
  const t = useT();
  const summary = testing ? t("servers.screen.checking") : t("servers.screen.onlineCount", { online, total: count });
  return (
    <Panel>
      <PanelHeader
        title={
          <>
            <StatusDot tone={tone} pulse={testing} />
            {t("servers.screen.hosts")}
          </>
        }
        meta={`${summary} · ${t("servers.screen.hostCount", { count })}`}
      />
      {children}
    </Panel>
  );
}

/** Points to setting up this machine, for when it can't run VM labs itself. */
function SetUpMachineHint({ onSetUp }: { onSetUp: () => void }) {
  const t = useT();
  return (
    <Panel>
      <div className="flex flex-wrap items-center gap-3 px-4 py-3">
        <StatusDot tone="muted" />
        <p className="min-w-0 flex-1 text-[0.8125rem] text-muted-foreground">{t("servers.screen.noServerHint")}</p>
        <Button variant="ghost" size="xs" onClick={onSetUp}>
          {t("servers.screen.setUpMachine")}
        </Button>
      </div>
    </Panel>
  );
}

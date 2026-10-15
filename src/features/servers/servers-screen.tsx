"use client";

import { useState, type ReactNode } from "react";
import { Plus, RefreshCw } from "lucide-react";
import { Panel, PanelHeader } from "@/components/ui/panel";
import { Button } from "@/components/ui/button";
import { NoticePanel } from "@/components/ui/notice-panel";
import { Skeleton } from "@/components/ui/skeleton";
import { Spinner } from "@/components/ui/spinner";
import { serverRemove, serverSetDefault, type ServerHost } from "@/lib/tauri";
import { PageHeader } from "@/components/ui/page-header";
import { HostRow, type HostActions } from "@/features/servers/host-row";
import { CapacityPanel } from "@/features/servers/capacity-panel";
import { ServersEmptyState } from "@/features/servers/servers-empty-state";
import { StatusDot, type Tone } from "@/components/ui/status-pill";
import { loadVmTests, saveVmTest, type VmTests } from "@/features/servers/vm-tests";
import { canRunVmHere, capacityHost, hostsSummary } from "@/features/servers/servers-model";
import { useServerHosts, type ServerHosts } from "@/features/servers/use-server-hosts";
import { useNow } from "@/lib/use-now";
import { useT } from "@/lib/i18n";

type Tab = "setup";

export function ServerScreen({ onNavigate }: { onNavigate: (tab: Tab) => void }) {
  const t = useT();
  const page = useServerHosts();
  const { hosts, report, error } = page;
  const summary = hostsSummary(hosts ?? [], page.tests);
  const capacity = capacityHost(hosts ?? [], page.defaultId, page.caps);
  const add = () => {
    page.open(null);
  };

  return (
    <div className="space-y-5">
      <PageHeader
        title={t("servers.screen.title")}
        lead={t("servers.screen.lead")}
        actions={
          <>
            {hosts && hosts.length > 0 && (
              <Button variant="outline" size="sm" onClick={page.testAll} disabled={summary.anyTesting}>
                {summary.anyTesting ? <Spinner className="size-3.5" /> : <RefreshCw className="size-3.5" />} {t("servers.screen.testAll")}
              </Button>
            )}
            <Button variant="outline" size="sm" onClick={add}>
              <Plus className="size-3.5" /> {t("servers.screen.addServer")}
            </Button>
          </>
        }
      />

      {error && <NoticePanel tone="fail">{error}</NoticePanel>}

      {hosts === null && <HostsLoading />}
      {hosts?.length === 0 && <ServersEmptyState onAdd={add} />}
      {hosts && hosts.length > 0 && (
        <HostsPanel count={hosts.length} online={summary.online} testing={summary.anyTesting} tone={summary.tone}>
          <HostRows page={page} hosts={hosts} />
        </HostsPanel>
      )}

      {capacity && <CapacityPanel host={capacity.host} cap={capacity.cap} />}

      {/* Only worth suggesting when this machine can't already run VM labs itself. */}
      {report && !canRunVmHere(report) && (
        <SetUpMachineHint
          onSetUp={() => {
            onNavigate("setup");
          }}
        />
      )}
    </div>
  );
}

/** A row per host, with the VM test open under at most one of them. */
function HostRows({ page, hosts }: { page: ServerHosts; hosts: ServerHost[] }) {
  const t = useT();
  const [vmTestId, setVmTestId] = useState<string | null>(null);
  const [vmResults, setVmResults] = useState<VmTests>(() => loadVmTests());
  // Relative "Xm ago" labels, refreshed on a slow tick.
  const [now] = useNow(30_000);
  const actionsFor = (h: ServerHost): HostActions => ({
    onTest: () => {
      void page.test(h.id);
    },
    onVmTest: () => {
      setVmTestId(vmTestId === h.id ? null : h.id);
    },
    onVmTestDone: (result) => {
      setVmResults(saveVmTest(h.id, result));
      void page.test(h.id);
      page.loadRunning();
    },
    onEdit: () => {
      page.open(h.id);
    },
    onDefault: () => {
      page.act(() => serverSetDefault(h.id === page.defaultId ? null : h.id));
    },
    onRemove: () => {
      if (confirm(t("servers.screen.removeConfirm", { name: h.name }))) page.act(() => serverRemove(h.id));
    },
  });
  return hosts.map((h) => (
    <HostRow
      key={h.id}
      host={h}
      now={now}
      actions={actionsFor(h)}
      view={{
        isDefault: h.id === page.defaultId,
        running: page.running[h.id] ?? 0,
        capacity: page.caps[h.id],
        test: page.tests[h.id],
        lastVm: vmResults[h.id],
        vmTesting: vmTestId === h.id,
      }}
    />
  ));
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

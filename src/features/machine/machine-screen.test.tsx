// @vitest-environment jsdom
import { act, render, renderHook, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";
import { DownloadsPanel } from "@/features/machine/downloads-panel";
import { DockerLabRow, VmLabRow } from "@/features/machine/lab-type-rows";
import { MachineDetails } from "@/features/machine/machine-details";
import { CalloutRow, StatCard } from "@/features/machine/machine-parts";
import { MachineScreen } from "@/features/machine/machine-screen";
import { vmSetup } from "@/features/machine/machine-status";
import { RunningNowPanel } from "@/features/machine/running-now-panel";
import { SelfTest } from "@/features/machine/self-test";
import { SetupChecklist } from "@/features/machine/setup-checklist";
import { StepRow } from "@/features/machine/step-row";
import { useMachineFormat } from "@/features/machine/use-machine-format";
import { LOCALES, setLocalePreference } from "@/lib/i18n";
import { setAttackImage, setLastTest, type LastTest } from "@/lib/settings";
import type { ProviderStatus, SelfTestEvent, SystemReport } from "@/lib/tauri";
import { freshReport, readyReport, stoppedDockerReport } from "@/test/fixtures";
import { commands, installTauri, stream } from "@/test/tauri";
import { button, flush } from "@/test/ui";

const GB = 1e9;
const text = () => document.body.textContent ?? "";
const hv = (over: Partial<ProviderStatus>): ProviderStatus =>
  ({ provider: "virtualbox", remote: false, available: true, hypervisor: true, plugin: null, pluginInstalled: true, reason: null, ...over }) as ProviderStatus;
const metrics = (over: Record<string, number> = {}) => ({
  cpu: 40,
  memUsed: 8 * GB,
  memTotal: 32 * GB,
  diskUsed: 100 * GB,
  diskTotal: 500 * GB,
  uptimeSecs: 3 * 86400 + 4 * 3600,
  cores: 8,
  containers: 0,
  ...over,
});

/* ------------------------------------------------------------------ format */

describe("useMachineFormat", () => {
  afterEach(() => {
    act(() => setLocalePreference("system"));
  });

  it("writes sizes, times and uptimes in English", () => {
    const { result } = renderHook(() => useMachineFormat());
    const f = result.current;
    expect(f.bytes(12 * GB)).toBe("12 GB");
    expect(f.bytes(3.42 * GB)).toBe("3.4 GB");
    expect(f.bytes(512e6)).toBe("512 MB");
    expect(f.bytes(8_000)).toBe("8 kB");
    expect(f.bytes(10)).toBe("1 kB");
    const now = 1_000_000_000_000;
    expect(f.ago(now - 10_000, now)).toBe("just now");
    expect(f.ago(now + 10_000, now)).toBe("just now");
    expect(f.ago(now - 5 * 60_000, now)).toBe("5 min ago");
    expect(f.ago(now - 3 * 3_600_000, now)).toBe("3 h ago");
    expect(f.ago(now - 86_400_000, now)).toBe("yesterday");
    expect(f.ago(now - 4 * 86_400_000, now)).toBe("4 days ago");
    expect(f.uptime(3 * 86400 + 4 * 3600)).toBe("3d 4h");
    expect(f.uptime(5 * 3600 + 12 * 60)).toBe("5h 12m");
    expect(f.uptime(42 * 60)).toBe("42m");
  });

  for (const locale of LOCALES) {
    it(`writes every unit in ${locale}`, () => {
      act(() => setLocalePreference(locale));
      const { result } = renderHook(() => useMachineFormat());
      const f = result.current;
      const now = 1_000_000_000_000;
      const out = [
        f.bytes(3.4 * GB),
        f.bytes(12 * GB),
        f.bytes(5e6),
        f.bytes(5e3),
        f.ago(now, now),
        f.ago(now - 300_000, now),
        f.ago(now - 7_200_000, now),
        f.ago(now - 86_400_000, now),
        f.ago(now - 3 * 86_400_000, now),
        f.uptime(90_000),
        f.uptime(3_700),
        f.uptime(60),
      ];
      for (const s of out) {
        expect(s).not.toMatch(/\{|\}|machine\.format/);
        expect(s.length).toBeGreaterThan(0);
      }
      // The number itself is always there, with the language's decimal separator.
      expect(out[0]).toMatch(locale === "en" || locale === "ja" ? /3\.4/ : /3,4/);
      expect(out[1]).toMatch(/12/);
      expect(out[9]).toMatch(/1/);
    });
  }
});

/* ------------------------------------------------------------------ small parts */

describe("machine parts", () => {
  it("shows a stat card's skeleton until it has a value, then a sparkline or a meter", () => {
    const { container, rerender } = render(<StatCard label="CPU" detail="" value={null} />);
    expect(container.textContent).toBe("CPU");
    rerender(<StatCard label="CPU" detail="8 cores" value={82.4} history={[10, 90]} />);
    expect(container.textContent).toContain("82");
    rerender(<StatCard label="CPU" detail="8 cores" value={20} />);
    expect(container.textContent).toContain("20");
    rerender(<StatCard label="Disk" detail="" value={null} meter />);
    rerender(<StatCard label="Disk" detail="" value={50} meter />);
    expect(container.textContent).toContain("50");
  });

  it("draws a callout with its meta, body and actions", () => {
    render(<CalloutRow tone="fail" title="Broken" meta="now" actions={<button>Fix</button>} />);
    expect(screen.getByRole("alert").textContent).toBe("Brokennow" + "Fix");
    render(
      <CalloutRow tone="warn" title="Careful" icon={<i>!</i>}>
        Some detail
      </CalloutRow>,
    );
    expect(text()).toContain("Some detail");
  });

  it("draws each step state, with a failing detail", () => {
    for (const state of ["done", "run", "fail", "pending", "skip"] as const)
      render(<StepRow state={state} label={state} detail={`${state} detail`} detailTone={state === "fail" ? "fail" : undefined} />);
    render(<StepRow state="done" label="with meta" meta="2s" action={<button>Go</button>} />);
    expect(text()).toContain("fail detail");
    expect(text()).toContain("2s");
  });
});

/* ------------------------------------------------------------------ self-test */

describe("SelfTest", () => {
  const ev = (step: string, state: SelfTestEvent["state"], detail?: string): SelfTestEvent =>
    ({ step, label: step, state, detail: detail ?? null }) as SelfTestEvent;

  it("runs on demand, ticks its steps and records a pass", async () => {
    const calls = installTauri({
      machine_selftest: (a) => {
        stream(
          a,
          ev("engine", "running"),
          ev("engine", "ok"),
          ev("pull", "ok"),
          ev("start", "ok"),
          ev("network", "skip"),
          ev("port", "ok"),
          ev("cleanup", "ok"),
        );
        return null;
      },
    });
    const onResult = vi.fn();
    const user = userEvent.setup();
    render(<SelfTest kind="docker" title="Container test" description="Pulls a tiny image" onResult={onResult} />);
    expect(screen.queryByRole("listitem")).toBeNull();
    await user.click(button(/run test/i));
    await flush();
    expect(calls.find((c) => c.cmd === "machine_selftest")?.args).toMatchObject({ kind: "docker", provider: null });
    expect(onResult.mock.calls.map((c) => c[0])).toEqual(["running", "ok"]);
    expect(text()).toContain("Passed");
    expect(text()).toContain("skipped");
    expect(screen.getAllByRole("listitem")).toHaveLength(6);
    expect(JSON.parse(localStorage.getItem("cyberctf.selftest.docker")!).result).toBe("ok");
    // And again.
    await user.click(screen.getByRole("button", { name: /run again/i }));
    await flush();
    expect(commands(calls).filter((c) => c === "machine_selftest")).toHaveLength(2);
  });

  it("runs the VM test on the chosen hypervisor and records a failure with its detail", async () => {
    localStorage.setItem("cyberctf.vm.provider", "qemu");
    const calls = installTauri({
      machine_selftest: (a) => {
        stream(a, ev("vagrant", "ok"), ev("provider", "fail", "vagrant-qemu is missing"));
        throw "provider check failed";
      },
    });
    const onResult = vi.fn();
    render(<SelfTest kind="vm" title="VM test" description="Boots a tiny VM" auto onResult={onResult} />);
    await flush();
    expect(calls.find((c) => c.cmd === "machine_selftest")?.args).toMatchObject({ kind: "vm", provider: "qemu" });
    expect(onResult).toHaveBeenLastCalledWith("fail");
    expect(text()).toContain("Failed");
    expect(text()).toContain("vagrant-qemu is missing");
    expect(screen.getAllByRole("listitem")).toHaveLength(8);
    expect(button(/retry/i)).toBeTruthy();
    expect(JSON.parse(localStorage.getItem("cyberctf.selftest.vm")!).result).toBe("fail");
  });

  it("counts passed steps while running", async () => {
    let finish: () => void = () => {};
    installTauri({
      machine_selftest: (a) => {
        stream(a, ev("engine", "ok"), ev("pull", "running"));
        return new Promise<null>((r) => (finish = () => r(null)));
      },
    });
    render(<SelfTest kind="docker" title="t" description="d" auto />);
    await flush();
    expect(text()).toContain("Testing");
    expect(text()).toMatch(/1\/6/);
    await act(async () => finish());
    await flush();
    expect(text()).toContain("Passed");
  });
});

/* ------------------------------------------------------------------ lab type rows */

describe("DockerLabRow", () => {
  const base = { now: Date.now(), memFree: 16 * GB, testing: false, onToggleTest: vi.fn(), onTestDone: vi.fn(), onFix: vi.fn() };

  it("shows a ready engine, its last test, and opens the test", async () => {
    installTauri();
    const onToggleTest = vi.fn();
    const user = userEvent.setup();
    render(<DockerLabRow {...base} onToggleTest={onToggleTest} report={readyReport()} last={{ result: "ok", at: base.now - 3_600_000 }} />);
    expect(text()).toContain("Ready");
    expect(text()).toContain("Docker Desktop · tested 1 h ago");
    await user.click(button(/^test$/i));
    expect(onToggleTest).toHaveBeenCalledOnce();
  });

  it("shows a failed last test and a low-memory hint", () => {
    installTauri();
    render(<DockerLabRow {...base} memFree={GB} report={{ ...readyReport(), dockerEngine: null }} last={{ result: "fail", at: Date.now() }} />);
    expect(text()).toContain("Test failed");
    expect(text()).toContain("Docker · last test failed just now");
    expect(text()).toContain("Container labs want about 2.0 GB of free memory; 1.0 GB free now.");
  });

  it("explains a denied, stopped or missing engine per OS, with a fix", async () => {
    installTauri();
    const onFix = vi.fn();
    const user = userEvent.setup();
    const { unmount } = render(<DockerLabRow {...base} report={stoppedDockerReport()} last={null} />);
    expect(text()).toContain("No permission");
    expect(text()).toContain("sudo usermod -aG docker $USER");
    expect(screen.queryByRole("button")).toBeNull();
    unmount();
    const denied = render(<DockerLabRow {...base} report={{ ...stoppedDockerReport(), dockerDeniedHint: null }} last={null} />);
    expect(text()).toContain("add yourself to the docker group");
    denied.unmount();
    const stopped = render(<DockerLabRow {...base} onFix={onFix} report={{ ...readyReport(), dockerRunning: false }} last={null} />);
    expect(text()).toContain("Engine stopped");
    await user.click(button(/fix/i));
    expect(onFix).toHaveBeenCalledWith("docker");
    stopped.unmount();
    for (const [os, msg] of [
      ["linux", /Docker Engine, Docker Desktop or Colima/],
      ["macos", /OrbStack/],
      ["windows", /Docker Desktop works\./],
    ] as const) {
      const r = render(<DockerLabRow {...base} report={freshReport(os)} last={null} />);
      expect(text()).toContain("Needs setup");
      expect(text()).toMatch(msg);
      r.unmount();
    }
  });

  it("shows the self-test inline and reports when it finishes", async () => {
    installTauri();
    const onTestDone = vi.fn();
    render(<DockerLabRow {...base} testing onTestDone={onTestDone} report={readyReport()} last={null} />);
    await flush();
    expect(text()).toContain("Hide test");
    expect(text()).toContain("not tested yet");
    expect(onTestDone).toHaveBeenCalledOnce();
  });
});

describe("VmLabRow", () => {
  const base = { now: Date.now(), memFree: 16 * GB, testing: false, onToggleTest: vi.fn(), onTestDone: vi.fn(), onFix: vi.fn(), onUseServer: vi.fn() };
  const row = (report: SystemReport, last: LastTest | null = null, over: Partial<typeof base> = {}) => (
    <VmLabRow {...base} {...over} report={report} last={last} vm={vmSetup(report, null, last)} />
  );

  it("shows a ready hypervisor, picked automatically among several", () => {
    installTauri();
    render(row({ ...readyReport(), os: "linux", vmProviders: [hv({}), hv({ provider: "vmware_desktop" })] }, { result: "ok", at: Date.now() }));
    expect(text()).toContain("Ready");
    expect(text()).toContain("VirtualBox (automatic, change in Settings) · tested just now");
  });

  it("offers a server beside the test when memory is short", async () => {
    installTauri();
    const onUseServer = vi.fn();
    const user = userEvent.setup();
    render(row({ ...readyReport(), vmProviders: [hv({})] }, { result: "fail", at: Date.now() }, { memFree: 2 * GB, onUseServer }));
    expect(text()).toContain("Test failed");
    expect(text()).toContain("VM labs want about 8.0 GB");
    await user.click(button(/use a server/i));
    expect(onUseServer).toHaveBeenCalledOnce();
    expect(button(/^test$/i)).toBeTruthy();
  });

  it("sends the player to a server when no local hypervisor runs here", async () => {
    installTauri();
    const onUseServer = vi.fn();
    const user = userEvent.setup();
    render(row({ ...readyReport(), vmProviders: [hv({ provider: "proxmox", remote: true })] }, null, { onUseServer }));
    expect(text()).toContain("Not on this machine");
    await user.click(button(/use a server/i));
    expect(onUseServer).toHaveBeenCalledOnce();
  });

  it("explains a blocked hypervisor", () => {
    installTauri();
    render(row({ ...readyReport(), vmProviders: [hv({ provider: "libvirt", available: false, reason: "no /dev/kvm" })] }));
    expect(text()).toContain("No /dev/kvm.");
    expect(button(/use a server/i)).toBeTruthy();
  });

  it("fixes a missing Vagrant or a missing hypervisor at the right step", async () => {
    installTauri();
    const onFix = vi.fn();
    const user = userEvent.setup();
    const r = render(row({ ...readyReport(), vagrant: { installed: false, version: null }, vmProviders: [hv({ available: false })] }, null, { onFix }));
    expect(text()).toContain("Vagrant or the hypervisor's Vagrant plugin is missing.");
    await user.click(button(/fix/i));
    expect(onFix).toHaveBeenLastCalledWith("vagrant");
    r.unmount();
    render(row({ ...readyReport(), os: "linux", vmProviders: [hv({ hypervisor: false, available: false })] }, null, { onFix }));
    expect(text()).toContain("Install a hypervisor.");
    await user.click(button(/fix/i));
    expect(onFix).toHaveBeenLastCalledWith("vm");
  });
});

/* ------------------------------------------------------------------ panels */

describe("DownloadsPanel", () => {
  it("lists the downloads and frees space after confirming", async () => {
    setAttackImage("kalilinux/kali-rolling");
    const calls = installTauri();
    const user = userEvent.setup();
    render(<DownloadsPanel />);
    await flush();
    expect(calls.find((c) => c.cmd === "machine_storage")?.args).toEqual({ extraImages: ["kalilinux/kali-rolling", "cyberctf/attack-box:latest"] });
    expect(text()).toContain("16 GB of lab downloads");
    expect(text()).toContain("2 container images · 1 VM image");
    expect(text()).not.toContain("postgres:16");
    await user.click(screen.getByRole("button", { expanded: false }));
    expect(text()).toContain("postgres:16");
    expect(text()).toContain("430 MB");
    await user.click(button(/clean up/i));
    expect(screen.getByRole("alert").textContent).toContain("Remove 16 GB?");
    await user.click(button(/cancel/i));
    expect(screen.queryByRole("alert")).toBeNull();
    await user.click(button(/clean up/i));
    await user.click(button(/^remove$/i));
    await flush();
    expect(calls.find((c) => c.cmd === "machine_storage_clean")?.args).toEqual({ extraImages: ["kalilinux/kali-rolling", "cyberctf/attack-box:latest"] });
    expect(text()).toContain("Freed 1.0 GB.");
    expect(commands(calls).filter((c) => c === "machine_storage")).toHaveLength(2);
  });

  it("says when nothing could be freed, or the clean-up failed", async () => {
    let clean: () => number = () => 0;
    installTauri({ machine_storage_clean: () => clean() });
    const user = userEvent.setup();
    render(<DownloadsPanel />);
    await flush();
    await user.click(button(/clean up/i));
    await user.click(button(/^remove$/i));
    await flush();
    expect(text()).toContain("Nothing could be removed (all in use).");
    clean = () => {
      throw "docker is not running";
    };
    await user.click(button(/clean up/i));
    await user.click(button(/^remove$/i));
    await flush();
    expect(text()).toContain("Nothing could be removed");
  });

  it("shows an empty list when there are no downloads or they can't be read", async () => {
    installTauri({
      machine_storage: () => {
        throw "docker is not running";
      },
    });
    render(<DownloadsPanel />);
    await flush();
    expect(text()).toContain("No lab downloads yet");
    expect(text()).toContain("0 container images · 0 VM images");
    expect(screen.queryByRole("button", { name: /clean up/i })).toBeNull();
    expect(screen.getByRole("button", { expanded: false }).hasAttribute("disabled")).toBe(true);
  });
});

describe("RunningNowPanel", () => {
  it("lists what runs, with lab titles, sizes and providers, and stops one", async () => {
    let workloads = [
      { id: "lab-invoice", kind: "docker", count: 3, memBytes: 0.6 * GB, provider: null },
      { id: "selftest", kind: "docker", count: 1, memBytes: 0, provider: null },
      { id: "lab-goad", kind: "vm", count: 1, memBytes: 4 * GB, provider: "virtualbox" },
      { id: "lab-x", kind: "vm", count: 2, memBytes: 0, provider: null },
    ];
    const calls = installTauri({
      machine_workloads: () => workloads,
      machine_workload_stop: (a) => {
        workloads = workloads.filter((w) => w.id !== a?.id);
        return null;
      },
    });
    const user = userEvent.setup();
    render(<RunningNowPanel refreshKey={1} />);
    await flush();
    expect(text()).toContain("4.6 GB in use");
    expect(text()).toContain("Invoice portal API");
    expect(text()).toContain("3 containers · 600 MB");
    expect(text()).toContain("Setup test");
    expect(text()).toContain("1 container");
    expect(text()).toContain("1 VM · VirtualBox");
    // No title known for it: its id, made readable.
    expect(screen.getByText("X")).toBeTruthy();
    expect(text()).toContain("2 VMs");
    const rows = screen.getAllByRole("button", { name: /stop/i });
    await user.click(rows[0]);
    await flush();
    expect(calls.find((c) => c.cmd === "machine_workload_stop")?.args).toEqual({ kind: "docker", id: "lab-invoice" });
    expect(text()).not.toContain("Invoice portal API");
  });

  it("says nothing runs, also when the list can't be read", async () => {
    installTauri({
      machine_workloads: () => {
        throw "docker is not running";
      },
    });
    render(<RunningNowPanel refreshKey={null} />);
    await flush();
    expect(text()).toContain("Nothing running.");
  });

  it("keeps the list after a failed stop, and shows readable lab ids without titles", async () => {
    const calls = installTauri({
      api_query: () => {
        throw "offline";
      },
      machine_workloads: () => [{ id: "lab-invoice", kind: "docker", count: 1, memBytes: 0, provider: null }],
      machine_workload_stop: () => {
        throw "container is stuck";
      },
    });
    const user = userEvent.setup();
    render(<RunningNowPanel refreshKey={null} />);
    await flush();
    expect(text()).toContain("Invoice");
    expect(text()).not.toContain("lab-invoice");
    await user.click(button(/stop/i));
    await flush();
    expect(text()).toContain("Invoice");
    expect(commands(calls).filter((c) => c === "machine_workloads").length).toBeGreaterThanOrEqual(2);
  });
});

describe("MachineDetails", () => {
  it("lists tool versions, hypervisors and targets", () => {
    const report: SystemReport = {
      ...readyReport(),
      os: "windows",
      dockerEngine: null,
      dockerRunning: true,
      docker: { installed: true, version: "Docker version 27.3.1, build ce12230" },
      dockerCompose: { installed: true, version: "no number" },
      targets: [
        { target: "terraform", cloud: "aws", ready: false, summary: "AWS CLI missing", notes: [] },
        { target: "docker", cloud: null, ready: true, summary: "Docker 27", notes: [] },
      ],
    } as SystemReport;
    const hypervisors = [
      hv({ provider: "vmware_desktop", plugin: "vagrant-vmware-desktop", pluginInstalled: true }),
      hv({ provider: "qemu", plugin: "vagrant-qemu", pluginInstalled: false }),
      hv({}),
      hv({ provider: "utm", hypervisor: false }),
      hv({ provider: "hyperv", hypervisor: null }),
    ];
    render(<MachineDetails report={report} hypervisors={hypervisors} hasHypervisor />);
    expect(text()).toContain("Container enginerunning");
    expect(text()).toContain("Docker CLI27.3.1");
    expect(text()).toContain("Docker Composeinstalled");
    expect(text()).toContain("VMware Workstationinstalled · vagrant-vmware-desktop");
    expect(text()).toContain("QEMUplugin vagrant-qemu missing");
    expect(text()).toContain("VirtualBoxinstalled");
    expect(text()).toContain("UTMnot installed");
    expect(text()).toContain("Hyper-Vbuilt in");
    expect(text()).toContain("terraform · aws");
    expect(text()).toContain("Where labs can run (Isoloom)");
  });

  it("shows a stopped engine and missing tools, without targets", () => {
    render(<MachineDetails report={freshReport("linux")} hypervisors={[]} hasHypervisor={false} />);
    expect(text()).toContain("Container enginenot running");
    expect(text()).toContain("Vagrantnot installed");
    expect(text()).toContain("aptnot installed");
    expect(text()).not.toContain("Isoloom");
  });
});

/* ------------------------------------------------------------------ setup checklist */

describe("SetupChecklist", () => {
  it("marks every step of a ready, tested machine done", async () => {
    const calls = installTauri();
    const user = userEvent.setup();
    const ok = { result: "ok" as const, at: Date.now() };
    render(<SetupChecklist report={readyReport()} last={{ docker: ok, vm: ok }} onFix={vi.fn()} />);
    expect(text()).toContain("7 of 7 done");
    expect(text()).toContain("Docker Desktop");
    expect(text()).toContain("Cyber CTF");
    expect(screen.getAllByRole("button")).toHaveLength(1);
    await user.click(button(/open guided setup/i));
    await flush();
    expect(calls.find((c) => c.cmd === "machine_open_setup")?.args).toEqual({ step: null });
  });

  it("lists what a fresh Windows PC still needs, with the right action per step", async () => {
    installTauri();
    setAttackImage("ghcr.io/me/box");
    const onFix = vi.fn();
    const user = userEvent.setup();
    render(<SetupChecklist report={freshReport("windows")} last={{ docker: null, vm: { result: "fail", at: Date.now() } }} onFix={onFix} />);
    expect(text()).toContain("1 of 8 done");
    expect(text()).toContain("not checked");
    expect(text()).toContain("ghcr.io/me/box");
    expect(text()).toContain("failed");
    expect(text()).toContain("optional");
    await user.click(screen.getAllByRole("button", { name: /^set up$/i })[0]);
    expect(onFix).toHaveBeenLastCalledWith("pkgmgr");
    await user.click(screen.getAllByRole("button", { name: /^test$/i })[0]);
    expect(onFix).toHaveBeenLastCalledWith("docker-test");
    await user.click(screen.getAllByRole("button", { name: /^add$/i })[0]);
    expect(onFix).toHaveBeenLastCalledWith("vm");
  });

  it("names a running engine it has no logo for, a stopped engine, a blocked hypervisor and a missing plugin", () => {
    installTauri();
    const report: SystemReport = {
      ...readyReport(),
      os: "linux",
      dockerEngine: null,
      vmProviders: [hv({ provider: "libvirt", available: false, reason: "no /dev/kvm", plugin: "vagrant-libvirt", pluginInstalled: true })],
    };
    const r = render(<SetupChecklist report={report} last={{ docker: null, vm: null }} onFix={vi.fn()} />);
    expect(text()).toContain("running");
    expect(text()).toContain("libvirt (KVM) · can't run here");
    r.unmount();
    render(
      <SetupChecklist
        report={{
          ...readyReport(),
          dockerRunning: false,
          vmProviders: [hv({ provider: "qemu", available: false, plugin: "vagrant-qemu", pluginInstalled: false })],
        }}
        last={{ docker: null, vm: null }}
        onFix={vi.fn()}
      />,
    );
    expect(text()).toContain("stopped");
    expect(text()).toContain("plugin vagrant-qemu missing");
  });
});

/* ------------------------------------------------------------------ the screen */

describe("MachineScreen", () => {
  afterEach(() => vi.useRealTimers());

  it("says the machine is ready, with its live usage", async () => {
    installTauri({ machine_metrics: () => metrics() });
    render(<MachineScreen report={readyReport()} onRefresh={vi.fn()} onNavigate={vi.fn()} />);
    await flush(50);
    expect(text()).toContain("Ready for labs");
    expect(text()).toContain("macOS arm64");
    expect(text()).toContain("up 3d 4h");
    expect(text()).toContain("8 cores");
    // Compact, so it fits the card's corner in every language.
    expect(text()).toContain("8.0/32 GB");
    expect(text()).not.toContain("Low disk space");
    expect(screen.queryByRole("button", { name: /set up this machine/i })).toBeNull();
  });

  it("warns about low disk space and counts what needs setup", async () => {
    const calls = installTauri({ machine_metrics: () => metrics({ diskUsed: 490 * GB }) });
    const user = userEvent.setup();
    render(<MachineScreen report={freshReport("linux")} onRefresh={vi.fn()} onNavigate={vi.fn()} />);
    await flush(50);
    expect(text()).toContain("Low disk space");
    expect(text()).toContain("10 GB free");
    expect(text()).toContain("2 lab types need setup");
    expect(text()).toContain("Linux x86_64");
    await user.click(button(/set up this machine/i));
    await flush();
    expect(calls.find((c) => c.cmd === "machine_open_setup")?.args).toEqual({ step: null });
    await user.click(screen.getAllByRole("button", { name: /^fix$/i })[0]);
    await flush();
    expect(calls.filter((c) => c.cmd === "machine_open_setup").at(-1)?.args).toEqual({ step: "docker" });
  });

  it("names an unknown OS as-is and counts one lab type", async () => {
    installTauri({ machine_metrics: () => metrics() });
    render(<MachineScreen report={{ ...readyReport(), os: "freebsd", dockerRunning: false }} onRefresh={vi.fn()} onNavigate={vi.fn()} />);
    await flush(50);
    expect(text()).toContain("freebsd arm64");
    expect(text()).toContain("1 lab type needs setup");
  });

  it("switches to the setup checklist and back", async () => {
    installTauri();
    const user = userEvent.setup();
    render(<MachineScreen report={readyReport()} onRefresh={vi.fn()} onNavigate={vi.fn()} />);
    await flush(50);
    await user.click(screen.getByRole("radio", { name: "Setup" }));
    expect(text()).toContain("Guided setup");
    expect(text()).not.toContain("Running now");
    await user.click(screen.getByRole("radio", { name: "Health" }));
    expect(text()).toContain("Running now");
  });

  it("re-reads the machine when the window regains focus, with the latest callback", async () => {
    installTauri();
    const first = vi.fn();
    const second = vi.fn();
    const { rerender } = render(<MachineScreen report={readyReport()} onRefresh={first} onNavigate={vi.fn()} />);
    rerender(<MachineScreen report={readyReport()} onRefresh={second} onNavigate={vi.fn()} />);
    act(() => void window.dispatchEvent(new Event("focus")));
    expect(first).not.toHaveBeenCalled();
    expect(second).toHaveBeenCalledOnce();
  });

  it("opens the container test inline and shows its result in the row", async () => {
    installTauri();
    const user = userEvent.setup();
    render(<MachineScreen report={readyReport()} onRefresh={vi.fn()} onNavigate={vi.fn()} />);
    await flush(50);
    const engines = screen.getByText("Engines").closest("section, div")!.parentElement!;
    await user.click(within(engines).getAllByRole("button", { name: /^test$/i })[0]);
    await flush(50);
    expect(text()).toContain("Hide test");
    expect(text()).toMatch(/tested just now/);
    await user.click(button(/hide test/i));
    expect(text()).not.toContain("Hide test");
  });

  it("sends VM labs to a server when no hypervisor runs here", async () => {
    installTauri();
    const onNavigate = vi.fn();
    const user = userEvent.setup();
    render(<MachineScreen report={{ ...readyReport(), vmProviders: [] }} onRefresh={vi.fn()} onNavigate={onNavigate} />);
    await flush(50);
    await user.click(button(/use a server/i));
    expect(onNavigate).toHaveBeenCalledWith("server");
  });

  it("shows the last saved test results", async () => {
    setLastTest("vm", "fail");
    installTauri();
    render(<MachineScreen report={readyReport()} onRefresh={vi.fn()} onNavigate={vi.fn()} />);
    await flush(50);
    expect(text()).toContain("last test failed just now");
  });
});

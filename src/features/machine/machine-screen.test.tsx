// @vitest-environment jsdom
import { act, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";
import { DownloadsPanel } from "@/features/machine/downloads-panel";
import { MachineDetails } from "@/features/machine/machine-details";
import { CalloutRow, StatCard } from "@/features/machine/machine-parts";
import { MachineScreen } from "@/features/machine/machine-screen";
import { EngineRows } from "@/features/machine/engine-rows";
import { HypervisorRows } from "@/features/machine/hypervisor-rows";
import { vmSetup } from "@/features/machine/machine-status";
import { RunningNowPanel } from "@/features/machine/running-now-panel";
import { SelfTest } from "@/features/machine/self-test";
import { StepRow } from "@/features/machine/step-row";
import { formatAgo, formatBytes, formatBytesOf, formatUptime } from "@/lib/format";
import { LOCALES, setLocalePreference } from "@/lib/i18n";
import { getLastTest, setAttackImage, setLastTest, type LastTest } from "@/lib/settings";
import type { SelfTestEvent, SystemReport } from "@/lib/tauri";
import type { SelfTestResult } from "@/features/machine/use-self-test";
import { freshReport, hypervisorStatus as hv, readyReport, stoppedDockerReport } from "@/test/fixtures";
import { commands, installTauri, rejectWith, stream } from "@/test/tauri";
import { button, flush, settle } from "@/test/ui";
import { pressFirstTest, renderMachineScreen } from "@/features/machine/machine-test-kit";

const GB = 1e9;
const text = () => document.body.textContent;
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

describe("the Machine page's sizes and times (lib/format, the common.units keys)", () => {
  afterEach(() => {
    act(() => {
      setLocalePreference("system");
    });
  });

  it("writes sizes, times and uptimes in English", () => {
    act(() => {
      setLocalePreference("en");
    });
    const f = { bytes: formatBytes, ago: formatAgo, uptime: formatUptime };
    expect(formatBytesOf(3.42 * GB, 8 * GB)).toBe("3.4/8.0 GB");
    expect(formatBytesOf(19.5 * GB, 32 * GB)).toBe("20/32 GB");
    expect(formatBytesOf(512e6, 32 * GB)).toBe("512 MB / 32 GB");
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
      act(() => {
        setLocalePreference(locale);
      });
      const f = { bytes: formatBytes, ago: formatAgo, uptime: formatUptime };
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
        expect(s).not.toMatch(/\{|\}|common\.units|common\.time/);
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
  const ev = (step: string, state: SelfTestEvent["state"], detail?: string): SelfTestEvent => ({ step, label: step, state, detail: detail ?? null });

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
    const onResult = vi.fn<(r: SelfTestResult) => void>();
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
    expect(getLastTest("docker")?.result).toBe("ok");
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
        rejectWith("provider check failed");
      },
    });
    const onResult = vi.fn<(r: SelfTestResult) => void>();
    render(<SelfTest kind="vm" title="VM test" description="Boots a tiny VM" auto onResult={onResult} />);
    await flush();
    expect(calls.find((c) => c.cmd === "machine_selftest")?.args).toMatchObject({ kind: "vm", provider: "qemu" });
    expect(onResult).toHaveBeenLastCalledWith("fail");
    expect(text()).toContain("Failed");
    expect(text()).toContain("vagrant-qemu is missing");
    expect(screen.getAllByRole("listitem")).toHaveLength(8);
    expect(button(/retry/i)).toBeTruthy();
    expect(getLastTest("vm")?.result).toBe("fail");
  });

  it("counts passed steps while running", async () => {
    let finish: () => void = () => {};
    installTauri({
      machine_selftest: (a) => {
        stream(a, ev("engine", "ok"), ev("pull", "running"));
        return new Promise<null>(
          (r) =>
            (finish = () => {
              r(null);
            }),
        );
      },
    });
    render(<SelfTest kind="docker" title="t" description="d" auto />);
    await flush();
    expect(text()).toContain("Testing");
    expect(text()).toMatch(/1\/6/);
    await settle(() => {
      finish();
    });
    await flush();
    expect(text()).toContain("Passed");
  });
});

/* ------------------------------------------------------------------ provider rows */

describe("EngineRows", () => {
  const base = { now: Date.now(), memFree: 16 * GB, testing: false, onToggleTest: vi.fn(), onTestDone: vi.fn(), onFix: vi.fn() };

  it("lists this OS's engines: the one in use with its last test and the test, the others to add", async () => {
    installTauri();
    const onToggleTest = vi.fn();
    const onFix = vi.fn();
    const user = userEvent.setup();
    render(<EngineRows {...base} onToggleTest={onToggleTest} onFix={onFix} report={readyReport()} last={{ result: "ok", at: base.now - 3_600_000 }} />);
    // The ready Mac: Docker Desktop in use; OrbStack and Colima not set up.
    expect(text()).toMatch(/Docker Desktop.*tested 1 h ago.*Ready/);
    expect(text()).toContain("OrbStack");
    expect(text()).toContain("Colima");
    expect(screen.getAllByText("Not set up")).toHaveLength(2);
    await user.click(button(/^test$/i));
    expect(onToggleTest).toHaveBeenCalledOnce();
    await user.click(screen.getAllByRole("button", { name: /^add$/i })[0]);
    expect(onFix).toHaveBeenCalledWith("docker");
  });

  it("shows a failed last test and a low-memory hint on the engine in use, and names one without a tile", () => {
    installTauri();
    render(<EngineRows {...base} memFree={GB} report={{ ...readyReport(), dockerEngine: "podman" }} last={{ result: "fail", at: Date.now() }} />);
    expect(text()).toMatch(/Podman.*last test failed just now.*Test failed/);
    expect(text()).toContain("Container labs want about 2.0 GB of free memory; 1.0 GB free now.");
  });

  it("marks another running engine, with the way to switch", async () => {
    installTauri();
    const onFix = vi.fn();
    const user = userEvent.setup();
    render(<EngineRows {...base} onFix={onFix} report={{ ...readyReport(), dockerEnginesRunning: ["docker-desktop", "orbstack"] }} last={null} />);
    expect(text()).toContain("Running. Labs use Docker Desktop; switch in the setup.");
    await user.click(button(/^fix$/i));
    expect(onFix).toHaveBeenCalledWith("docker");
  });

  it("explains a denied or stopped engine on the OS's default, and offers every engine on a fresh machine", async () => {
    installTauri();
    const onFix = vi.fn();
    const user = userEvent.setup();
    const { unmount } = render(<EngineRows {...base} report={stoppedDockerReport()} last={null} />);
    expect(text()).toMatch(/Docker Engine.*No permission/);
    expect(text()).toContain("sudo usermod -aG docker player");
    expect(screen.queryByRole("button", { name: /^fix$/i })).toBeNull();
    unmount();
    // Said in the player's language, from the hint's code (not the core's English sentence).
    act(() => {
      setLocalePreference("fr");
    });
    const fr = render(<EngineRows {...base} report={stoppedDockerReport()} last={null} />);
    expect(text()).toContain("Ajoutez-vous au groupe docker dans un terminal (sudo usermod -aG docker player)");
    fr.unmount();
    act(() => {
      setLocalePreference("en");
    });
    const denied = render(<EngineRows {...base} report={{ ...stoppedDockerReport(), dockerDeniedHint: null }} last={null} />);
    expect(text()).toContain("add yourself to the docker group");
    denied.unmount();
    const stopped = render(<EngineRows {...base} onFix={onFix} report={{ ...readyReport(), dockerRunning: false }} last={null} />);
    expect(text()).toMatch(/Docker Desktop.*Engine stopped/);
    await user.click(button(/^fix$/i));
    expect(onFix).toHaveBeenCalledWith("docker");
    stopped.unmount();
    for (const [os, names] of [
      ["linux", ["Docker Desktop", "Docker Engine", "Colima"]],
      ["macos", ["Docker Desktop", "OrbStack", "Colima"]],
      ["windows", ["Docker Desktop"]],
    ] as const) {
      const r = render(<EngineRows {...base} report={freshReport(os)} last={null} />);
      for (const name of names) expect(text(), os).toContain(name);
      expect(screen.getAllByText("Not set up"), os).toHaveLength(names.length);
      expect(screen.queryByText("Ready")).toBeNull();
      r.unmount();
    }
  });

  it("shows the self-test inline and reports when it finishes", async () => {
    installTauri();
    const onTestDone = vi.fn();
    render(<EngineRows {...base} testing onTestDone={onTestDone} report={readyReport()} last={null} />);
    await flush();
    expect(text()).toContain("Hide test");
    expect(text()).toContain("not tested yet");
    expect(onTestDone).toHaveBeenCalledOnce();
  });
});

describe("HypervisorRows", () => {
  const base = { now: Date.now(), memFree: 16 * GB, testing: false, onToggleTest: vi.fn(), onTestDone: vi.fn(), onFix: vi.fn(), onUseServer: vi.fn() };
  const rows = (report: SystemReport, last: LastTest | null = null, over: Partial<typeof base> = {}) => (
    <HypervisorRows {...base} {...over} report={report} last={last} vm={vmSetup(report, null, last)} />
  );

  it("shows the hypervisor labs run on, picked automatically among several, and the other one installed", () => {
    installTauri();
    render(rows({ ...readyReport(), os: "linux", vmProviders: [hv({}), hv({ provider: "vmware_desktop" })] }, { result: "ok", at: Date.now() }));
    expect(text()).toMatch(/VirtualBox.*\(automatic, change in Settings\) · tested just now.*Ready/);
    expect(text()).toMatch(/VMware Workstation.*Labs run on VirtualBox; change it in Settings\..*Installed/);
  });

  it("offers a server beside the test when memory is short", async () => {
    installTauri();
    const onUseServer = vi.fn();
    const user = userEvent.setup();
    render(rows({ ...readyReport(), vmProviders: [hv({})] }, { result: "fail", at: Date.now() }, { memFree: 2 * GB, onUseServer }));
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
    render(rows({ ...readyReport(), vmProviders: [hv({ provider: "proxmox", remote: true })] }, null, { onUseServer }));
    expect(text()).toContain("Not on this machine");
    await user.click(button(/use a server/i));
    expect(onUseServer).toHaveBeenCalledOnce();
  });

  it("explains a blocked hypervisor", () => {
    installTauri();
    render(rows({ ...readyReport(), vmProviders: [hv({ provider: "libvirt", available: false, reason: "no /dev/kvm" })] }));
    expect(text()).toMatch(/libvirt \(KVM\).*No \/dev\/kvm\..*Can't run here/);
    expect(button(/use a server/i)).toBeTruthy();
  });

  it("fixes a missing Vagrant at its step, and adds a hypervisor not installed at its own", async () => {
    installTauri();
    const onFix = vi.fn();
    const user = userEvent.setup();
    const r = render(rows({ ...readyReport(), vagrant: { installed: false, version: null }, vmProviders: [hv({ available: false })] }, null, { onFix }));
    expect(text()).toContain("Vagrant or the hypervisor's Vagrant plugin is missing.");
    await user.click(button(/^fix$/i));
    expect(onFix).toHaveBeenLastCalledWith("vagrant");
    r.unmount();
    render(
      rows(
        {
          ...readyReport(),
          os: "linux",
          vmProviders: [hv({ hypervisor: false, available: false }), hv({ provider: "vmware_desktop", hypervisor: false, available: false })],
        },
        null,
        { onFix },
      ),
    );
    expect(text()).toMatch(/VirtualBox.*Cyber CTF can install it for you\..*Not installed/);
    expect(text()).toMatch(/VMware Workstation.*Install it from the vendor's site\./);
    await user.click(screen.getAllByRole("button", { name: /^add$/i })[0]);
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
      rejectWith("docker is not running");
    };
    await user.click(button(/clean up/i));
    await user.click(button(/^remove$/i));
    await flush();
    expect(text()).toContain("Nothing could be removed");
  });

  it("shows an empty list when there are no downloads or they can't be read", async () => {
    installTauri({
      machine_storage: () => {
        rejectWith("docker is not running");
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
        rejectWith("docker is not running");
      },
    });
    render(<RunningNowPanel refreshKey={null} />);
    await flush();
    expect(text()).toContain("Nothing running.");
  });

  it("keeps the list after a failed stop, and shows readable lab ids without titles", async () => {
    const calls = installTauri({
      api_query: () => {
        rejectWith("offline");
      },
      machine_workloads: () => [{ id: "lab-invoice", kind: "docker", count: 1, memBytes: 0, provider: null }],
      machine_workload_stop: () => {
        rejectWith("container is stuck");
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
  it("lists tool versions and hypervisors", () => {
    const report: SystemReport = {
      ...readyReport(),
      os: "windows",
      dockerEngine: null,
      dockerRunning: true,
      docker: { installed: true, version: "Docker version 27.3.1, build ce12230" },
      dockerCompose: { installed: true, version: "no number" },
    };
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
  });

  it("shows a stopped engine and missing tools", () => {
    render(<MachineDetails report={freshReport("linux")} hypervisors={[]} hasHypervisor={false} />);
    expect(text()).toContain("Container enginenot running");
    expect(text()).toContain("Vagrantnot installed");
    expect(text()).toContain("aptnot installed");
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
    // The providers, each its own row; "Add a provider" opens the setup on its choice.
    expect(text()).toContain("Providers");
    expect(text()).toMatch(/Docker Desktop.*OrbStack.*Colima.*VirtualBox/);
    expect(screen.getByRole("button", { name: /add a provider/i })).toBeTruthy();
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
    await user.click(button(/add a provider/i));
    await flush();
    expect(calls.find((c) => c.cmd === "machine_open_setup")?.args).toEqual({ step: null });
    // A row's Add opens the setup at that provider's step.
    await user.click(screen.getAllByRole("button", { name: /^add$/i })[0]);
    await flush();
    expect(calls.filter((c) => c.cmd === "machine_open_setup").at(-1)?.args).toEqual({ step: "docker" });
    const adds = screen.getAllByRole("button", { name: /^add$/i });
    await user.click(adds[adds.length - 1]);
    await flush();
    expect(calls.filter((c) => c.cmd === "machine_open_setup").at(-1)?.args).toEqual({ step: "vm" });
  });

  it("names an unknown OS as-is and counts one lab type", async () => {
    installTauri({ machine_metrics: () => metrics() });
    render(<MachineScreen report={{ ...readyReport(), os: "freebsd", dockerRunning: false }} onRefresh={vi.fn()} onNavigate={vi.fn()} />);
    await flush(50);
    expect(text()).toContain("freebsd arm64");
    expect(text()).toContain("1 lab type needs setup");
  });

  it("re-reads the machine when the window regains focus, with the latest callback", () => {
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
    await renderMachineScreen();
    await pressFirstTest(user);
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

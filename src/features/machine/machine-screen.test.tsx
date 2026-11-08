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
import { getLastTest, getVmProvider, setAttackImage, setLastTest, setVmProvider, type LastTest } from "@/lib/settings";
import type { SelfTestEvent, SystemReport } from "@/lib/tauri";
import type { SelfTestResult } from "@/features/machine/use-self-test";
import { freshReport, hypervisorStatus as hv, readyReport, stoppedDockerReport } from "@/test/fixtures";
import { commands, installTauri, rejectWith, stream } from "@/test/tauri";
import { button, flush, settle } from "@/test/ui";
import { pressFirstTest, renderMachineScreen } from "@/features/machine/machine-test-kit";
import { must } from "@/components/test-support";

const GB = 1e9;
const text = () => document.body.textContent;
/** A self-test event as the core streams it (the label as the step's name). */
const ev = (step: string, state: SelfTestEvent["state"], detail?: string): SelfTestEvent => ({ step, label: step, state, detail: detail ?? null });
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

/** What every provider row takes besides the report: now, free memory, no test open, spies. */
const rowCallbacks = () => ({
  now: Date.now(),
  memFree: 16 * GB,
  testing: null as string | null,
  onToggleTest: vi.fn(),
  onTestDone: vi.fn(),
  onFix: vi.fn(),
  onRefresh: vi.fn(),
});

describe("EngineRows", () => {
  const base = rowCallbacks();
  type Engine = SystemReport["dockerEngines"][number];
  const found = (engine: Engine["engine"], over: Partial<Engine> = {}): Engine => ({ engine, context: engine, running: true, current: false, ...over });

  it("lists every engine found, each with its own state and test, the current one marked with the labs' last test", async () => {
    const calls = installTauri();
    const onToggleTest = vi.fn();
    const onFix = vi.fn();
    const onRefresh = vi.fn();
    const user = userEvent.setup();
    const report: SystemReport = {
      ...readyReport(),
      dockerEngine: "orbstack",
      dockerEnginesRunning: ["orbstack", "docker-desktop"],
      dockerEngines: [found("orbstack", { current: true }), found("docker-desktop", { context: "desktop-linux" }), found("colima", { running: false })],
    };
    // Docker Desktop's own last test, failed: shown on its row with why, not counted against the labs.
    setLastTest("docker", "fail", { provider: "docker-desktop", primary: false, why: "pull timed out" });
    render(
      <EngineRows
        {...base}
        onToggleTest={onToggleTest}
        onFix={onFix}
        onRefresh={onRefresh}
        report={report}
        last={{ result: "ok", at: base.now - 3_600_000 }}
      />,
    );
    // Three rows, in the check's order: the current one, another running, one stopped.
    expect(text()).toMatch(
      /OrbStack.*current.*tested 1 h ago.*Ready.*Docker Desktop.*Labs use OrbStack by default\. · last test failed just now · pull timed out.*Running.*Colima.*Engine stopped/,
    );
    expect(screen.getAllByText("current")).toHaveLength(1);
    // Not installed here: not a row (Add a provider covers it).
    expect(text()).not.toContain("Podman");
    // Every running engine has its own test; the stopped one its fix.
    const tests = screen.getAllByRole("button", { name: /^test$/i });
    expect(tests).toHaveLength(2);
    await user.click(tests[0]);
    expect(onToggleTest).toHaveBeenLastCalledWith("docker:orbstack");
    await user.click(tests[1]);
    expect(onToggleTest).toHaveBeenLastCalledWith("docker:docker-desktop");
    await user.click(button(/^fix$/i));
    expect(onFix).toHaveBeenCalledWith("docker");
    // The other running engine can become the one labs use: Docker is pointed at it, the machine re-read.
    await user.click(button(/use by default/i));
    await flush();
    expect(calls.find((c) => c.cmd === "docker_use_engine")?.args).toEqual({ engine: "docker-desktop" });
    expect(onRefresh).toHaveBeenCalledOnce();
  });

  it("runs another engine's test on its own Docker context and keeps the result under that engine", async () => {
    const calls = installTauri({
      machine_selftest: (a) => {
        stream(a, ev("engine", "ok"), ev("pull", "fail", "toomanyrequests: rate limit"));
        rejectWith("pull failed");
      },
    });
    const onTestDone = vi.fn();
    const report: SystemReport = {
      ...readyReport(),
      dockerEnginesRunning: ["docker-desktop", "orbstack"],
      dockerEngines: [found("docker-desktop", { context: "desktop-linux", current: true }), found("orbstack")],
    };
    render(<EngineRows {...base} testing="docker:orbstack" onTestDone={onTestDone} report={report} last={null} />);
    await flush();
    expect(calls.find((c) => c.cmd === "machine_selftest")?.args).toMatchObject({ kind: "docker", provider: null, context: "orbstack" });
    expect(onTestDone).toHaveBeenCalledOnce();
    expect(getLastTest("docker", "orbstack")).toMatchObject({ result: "fail", why: "toomanyrequests: rate limit" });
    // OrbStack isn't what labs run on: the container labs' own last test is untouched.
    expect(getLastTest("docker")).toBeNull();
    expect(text()).toContain("Hide test");
  });

  it("shows a failed last test and a low-memory hint on the current engine, and names one without a tile", () => {
    installTauri();
    const report: SystemReport = {
      ...readyReport(),
      dockerEngine: "podman",
      dockerEngines: [found("podman", { context: "podman-machine-default", current: true })],
    };
    render(<EngineRows {...base} memFree={GB} report={report} last={{ result: "fail", at: Date.now() }} />);
    expect(text()).toMatch(/Podman.*current.*last test failed just now.*Test failed/);
    expect(text()).toContain("Container labs want about 2.0 GB of free memory; 1.0 GB free now.");
  });

  it("explains a denied or stopped current engine, and shows nothing when none is found", async () => {
    installTauri();
    const onFix = vi.fn();
    const user = userEvent.setup();
    const { unmount } = render(<EngineRows {...base} report={stoppedDockerReport()} last={null} />);
    expect(text()).toMatch(/Docker Engine.*current.*No permission/);
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
    const ready = readyReport();
    const stopped = render(
      <EngineRows
        {...base}
        onFix={onFix}
        report={{ ...ready, dockerRunning: false, dockerEngines: ready.dockerEngines.map((e) => ({ ...e, running: false })) }}
        last={null}
      />,
    );
    expect(text()).toMatch(/Docker Desktop.*current.*Engine stopped/);
    await user.click(button(/^fix$/i));
    expect(onFix).toHaveBeenCalledWith("docker");
    stopped.unmount();
    // Nothing found: no row at all (Add a provider, in the header, is the way to one).
    const { container } = render(<EngineRows {...base} onFix={onFix} report={freshReport("linux")} last={null} />);
    expect(container.textContent).toBe("");
  });

  it("shows the self-test inline and reports when it finishes", async () => {
    installTauri();
    const onTestDone = vi.fn();
    render(<EngineRows {...base} testing="docker:docker-desktop" onTestDone={onTestDone} report={readyReport()} last={null} />);
    await flush();
    expect(text()).toContain("Hide test");
    expect(text()).toContain("not tested yet");
    expect(onTestDone).toHaveBeenCalledOnce();
  });
});

describe("HypervisorRows", () => {
  const base = { ...rowCallbacks(), onUseServer: vi.fn() };
  const rows = (report: SystemReport, last: LastTest | null = null, over: Partial<typeof base> = {}) => (
    <HypervisorRows {...base} {...over} report={report} last={last} vm={vmSetup(report, null, last)} />
  );

  it("lists every hypervisor installed, each with its own state, the one labs run on marked", () => {
    installTauri();
    const report = {
      ...readyReport(),
      os: "linux",
      vmProviders: [
        hv({}),
        hv({ provider: "vmware_desktop" }),
        hv({ provider: "libvirt", available: false, reason: "no /dev/kvm" }),
        hv({ provider: "qemu", hypervisor: false }),
      ],
    };
    render(rows(report, { result: "ok", at: Date.now() }));
    // Picked automatically among the ready ones, and marked; the others each as they stand.
    expect(text()).toMatch(/VirtualBox.*default.*\(automatic, change in Settings\) · tested just now.*Ready/);
    expect(text()).toMatch(/VMware Workstation.*Labs run on VirtualBox by default\. · not tested yet.*Installed/);
    expect(text()).toMatch(/libvirt \(KVM\).*No \/dev\/kvm\..*Can't run here/);
    expect(screen.getAllByText("default")).toHaveLength(1);
    // Not installed: not a row (Add a provider covers it).
    expect(text()).not.toContain("QEMU");
    // Every ready hypervisor has its own test (not the blocked one); the other can become the default.
    expect(screen.getAllByRole("button", { name: /^test$/i })).toHaveLength(2);
    expect(screen.getAllByRole("button", { name: /use by default/i })).toHaveLength(1);
  });

  it("tests each hypervisor on its own, keeps its result on its row, and makes it the default from the row", async () => {
    const calls = installTauri({
      machine_selftest: (a) => {
        stream(a, ev("vagrant", "ok"), ev("provider", "fail", "no box for vmware_desktop"));
        rejectWith("provider check failed");
      },
    });
    const onToggleTest = vi.fn();
    const onTestDone = vi.fn();
    const user = userEvent.setup();
    const report = { ...readyReport(), os: "linux", vmProviders: [hv({}), hv({ provider: "vmware_desktop" })] };
    const r = render(rows(report, null, { onToggleTest }));
    const tests = screen.getAllByRole("button", { name: /^test$/i });
    await user.click(tests[1]);
    expect(onToggleTest).toHaveBeenLastCalledWith("vm:vmware_desktop");
    r.unmount();
    // Open: the VM test runs on that hypervisor, and its failure stays with its row (the page
    // re-reads the rows when a test ends; here, a re-render).
    const open = () => rows(report, null, { testing: "vm:vmware_desktop", onTestDone });
    const opened = render(open());
    await flush();
    expect(calls.find((c) => c.cmd === "machine_selftest")?.args).toMatchObject({ kind: "vm", provider: "vmware_desktop" });
    expect(onTestDone).toHaveBeenCalledOnce();
    expect(getLastTest("vm", "vmware_desktop")).toMatchObject({ result: "fail", why: "no box for vmware_desktop" });
    expect(getLastTest("vm")).toBeNull();
    opened.rerender(open());
    expect(text()).toMatch(/VMware Workstation.*last test failed just now · no box for vmware_desktop/);
    // And from the row, the Settings choice.
    await user.click(button(/use by default/i));
    expect(getVmProvider()).toBe("vmware_desktop");
  });

  it("marks the hypervisor chosen in Settings, not the first installed one", () => {
    installTauri();
    setVmProvider("vmware_desktop");
    const report = { ...readyReport(), os: "linux", vmProviders: [hv({}), hv({ provider: "vmware_desktop" })] };
    render(<HypervisorRows {...base} report={report} last={null} vm={vmSetup(report, "vmware_desktop", null)} />);
    expect(text()).toMatch(
      /VirtualBox.*Labs run on VMware Workstation by default\. · not tested yet.*Installed.*VMware Workstation.*default.*not tested yet.*Ready/,
    );
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

  it("explains a blocked hypervisor", () => {
    installTauri();
    render(rows({ ...readyReport(), vmProviders: [hv({ provider: "libvirt", available: false, reason: "no /dev/kvm" })] }));
    expect(text()).toMatch(/libvirt \(KVM\).*No \/dev\/kvm\..*Can't run here/);
    expect(button(/use a server/i)).toBeTruthy();
  });

  it("fixes a missing Vagrant at its step, and shows nothing with no hypervisor installed", async () => {
    installTauri();
    const onFix = vi.fn();
    const user = userEvent.setup();
    const r = render(rows({ ...readyReport(), vagrant: { installed: false, version: null }, vmProviders: [hv({ available: false })] }, null, { onFix }));
    expect(text()).toContain("Vagrant or the hypervisor's Vagrant plugin is missing.");
    await user.click(button(/^fix$/i));
    expect(onFix).toHaveBeenLastCalledWith("vagrant");
    r.unmount();
    // None installed, or none that applies here: no row, no placeholder.
    for (const vmProviders of [[hv({ hypervisor: false, available: false }), hv({ provider: "vmware_desktop", hypervisor: false, available: false })], []]) {
      const { container, unmount } = render(rows({ ...readyReport(), os: "linux", vmProviders }, null, { onFix }));
      expect(container.textContent).toBe("");
      unmount();
    }
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
  it("lists tool versions and the hypervisors installed", () => {
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
    // Not installed: not listed (Hyper-V, a Windows feature, is built in).
    expect(text()).not.toContain("UTM");
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
    expect(text()).toMatch(/Docker Desktop.*current.*VirtualBox.*default/);
    expect(text()).not.toContain("OrbStack");
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
    // Nothing installed: no provider rows, one line pointing at the header's button.
    expect(text()).toContain("Nothing installed yet.");
    expect(screen.queryByRole("button", { name: /^(fix|test|add)$/i })).toBeNull();
    await user.click(button(/add a provider/i));
    await flush();
    expect(calls.find((c) => c.cmd === "machine_open_setup")?.args).toEqual({ step: "add" });
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

  it("lists only what is installed: no rows for the providers this machine lacks", async () => {
    installTauri();
    const report = { ...readyReport(), vmProviders: readyReport().vmProviders.filter((p) => p.remote) };
    render(<MachineScreen report={report} onRefresh={vi.fn()} onNavigate={vi.fn()} />);
    await flush(50);
    const providers = must(screen.getByText("Providers").closest("section, div")?.parentElement, "the Providers panel").textContent;
    expect(providers).toContain("Docker Desktop");
    expect(providers).not.toContain("Nothing installed yet.");
    for (const name of ["OrbStack", "Colima", "VirtualBox", "VMware", "UTM", "Not on this machine", "Use a server"]) expect(providers).not.toContain(name);
  });

  it("shows the last saved test results", async () => {
    setLastTest("vm", "fail");
    installTauri();
    render(<MachineScreen report={readyReport()} onRefresh={vi.fn()} onNavigate={vi.fn()} />);
    await flush(50);
    expect(text()).toContain("last test failed just now");
  });
});

// @vitest-environment jsdom
import { act, render, renderHook, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";
import { AttackBoxPanel } from "@/features/labs/attack-box-panel";
import { Toaster } from "@/components/ui/toaster";
import { useAttackBoxAutoStart } from "@/features/labs/attack-box-auto";
import { useAttackBox, type AttackBox } from "@/features/labs/use-attack-box";
import { attachAttackBox } from "@/features/labs/attack-box-attach";
import type { ActiveOperation, AttackStatus, Runtime } from "@/lib/tauri";
import { DEFAULT_ATTACK_IMAGE, setAttackImage, setAutoAttackBox } from "@/lib/settings";
import { commands, installTauri, stream, type Call } from "@/test/tauri";
import { flush } from "@/test/ui";
import { rejectWith } from "@/features/labs/labs-test-kit";
import { setCatalogue } from "@/features/labs/lab-store";
import type { Lab } from "@/features/labs/use-labs";

const box = (p: Partial<AttackStatus> = {}): AttackStatus => ({
  reachable: true,
  running: false,
  ip: "",
  labNetwork: "default",
  networks: ["default"],
  defaultNetwork: "default",
  vm: false,
  ...p,
});
const count = (calls: Call[], cmd: string) => calls.filter((c) => c.cmd === cmd).length;

afterEach(() => {
  vi.useRealTimers();
  setAutoAttackBox(true);
});

describe("useAttackBox", () => {
  it("reads the container attack box for the image in Settings and polls it while the lab runs", async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true, toFake: ["setTimeout", "clearTimeout"] });
    setAttackImage("my/exegol:full");
    const calls = installTauri({ attack_box_status: () => box({ running: true, ip: "10.42.0.9" }) });
    const { result, rerender } = renderHook((p: { running: boolean }) => useAttackBox("ab-poll", { running: p.running }), {
      initialProps: { running: true },
    });
    await flush();
    expect(calls.find((c) => c.cmd === "attack_box_status")?.args).toEqual({ id: "ab-poll", runtime: "DOCKER" });
    expect(result.current.status?.ip).toBe("10.42.0.9");
    expect(result.current.name).toBe("my/exegol:full");
    act(() => {
      vi.advanceTimersByTime(5000);
    });
    await flush();
    expect(count(calls, "attack_box_status")).toBe(2);
    // The lab stops: no more reads, and the last status is dropped.
    rerender({ running: false });
    await flush();
    expect(result.current.status).toBeNull();
    act(() => {
      vi.advanceTimersByTime(15000);
    });
    await flush();
    expect(count(calls, "attack_box_status")).toBe(2);
  });

  it("doesn't read the box again when the learner picks the network it joins", async () => {
    const calls = installTauri({ attack_box_status: () => box({ networks: ["default", "dmz"] }) });
    const { result } = renderHook(() => useAttackBox("ab-pick", { running: true }));
    await flush();
    expect(count(calls, "attack_box_status")).toBe(1);
    act(() => {
      result.current.setNetwork("dmz");
    });
    await flush();
    expect(result.current.network).toBe("dmz");
    expect(count(calls, "attack_box_status")).toBe(1);
  });

  it("stops reading, and never attaches, a box the backend says it can't reach (a lab on an ESXi host)", async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true, toFake: ["setTimeout", "clearTimeout"] });
    const calls = installTauri({ attack_box_status: () => box({ reachable: false, networks: [], defaultNetwork: "" }) });
    renderHook(() => useAttackBox("ab-remote", { running: true }));
    await flush();
    act(() => {
      vi.advanceTimersByTime(15000);
    });
    await flush();
    expect(count(calls, "attack_box_status")).toBe(1);
    expect(count(calls, "attack_box_attach")).toBe(0);
  });

  it("leaves the image to the backend's default when the learner saved none", async () => {
    setAttackImage(DEFAULT_ATTACK_IMAGE);
    const calls = installTauri({ attack_box_status: () => box(), attack_box_attach: () => null });
    const { result } = renderHook(() => useAttackBox("ab-default", { running: true }));
    await flush();
    expect(result.current.name).toBe("cyberctf/attack-box");
    await act(() => result.current.start());
    expect(calls.find((c) => c.cmd === "attack_box_attach")?.args).toMatchObject({ pick: { image: null } });
  });

  it("names the Kali VM the launcher adds to a lab running as VMs, and takes it out", async () => {
    // A container lab run as one VM per machine: declared DOCKER, a VM here. The box is Kali there
    // (the image only goes to a container box).
    setAttackImage("my/exegol:full");
    const calls = installTauri({ attack_box_status: () => box({ vm: true }), attack_box_attach: () => null });
    const { result } = renderHook(() => useAttackBox("ab-as-vms", { running: true, runtime: "DOCKER" }));
    await flush();
    expect(result.current.vm).toBe(true);
    expect(result.current.name).toBe("Kali Linux");
    await act(() => result.current.start());
    expect(calls.find((c) => c.cmd === "attack_box_attach")?.args).toMatchObject({ id: "ab-as-vms", runtime: "DOCKER", pick: { image: "my/exegol:full" } });
    await act(() => result.current.stop());
    expect(calls.find((c) => c.cmd === "attack_box_detach")?.args).toMatchObject({ id: "ab-as-vms", runtime: "DOCKER" });
  });

  it("never attaches the box on its own, Settings or not: the app-wide auto-start does", async () => {
    const calls = installTauri({ attack_box_status: () => box() });
    renderHook(() => useAttackBox("ab-auto", { running: true }));
    await flush(50);
    expect(count(calls, "attack_box_status")).toBe(1);
    expect(count(calls, "attack_box_attach")).toBe(0);
  });

  it("doesn't touch the box while a lab operation is in flight", async () => {
    const calls = installTauri({ attack_box_status: () => box() });
    renderHook(() => useAttackBox("ab-hold", { running: true, holding: true }));
    await flush(50);
    expect(count(calls, "attack_box_status")).toBe(0);
    expect(count(calls, "attack_box_attach")).toBe(0);
  });

  it("shows a detached box at once and puts a failure on the log", async () => {
    let running = true;
    installTauri({
      attack_box_status: () => box({ running, ip: running ? "10.42.0.9" : "" }),
      attack_box_detach: (a) => {
        stream(a, "Taking the attack box out");
        running = false;
        return null;
      },
      attack_box_attach: () => rejectWith("image pull failed"),
    });
    const { result } = renderHook(() => useAttackBox("ab-stop", { running: true }));
    await flush();
    expect(result.current.status?.running).toBe(true);
    await act(() => result.current.stop());
    expect(result.current.status).toMatchObject({ running: false, ip: "" });
    expect(result.current.log).toEqual(["Removing the attack box…", "Taking the attack box out"]);
    await act(() => result.current.start());
    expect(result.current.log).toEqual(["Starting the attack box…", "✗ image pull failed"]);
    expect(result.current.busy).toBe(false);
  });

  it("keeps the last status when a read fails", async () => {
    let fail = false;
    vi.useFakeTimers({ shouldAdvanceTime: true, toFake: ["setTimeout", "clearTimeout"] });
    installTauri({
      attack_box_status: () => {
        if (fail) throw new Error("runner busy");
        return box({ running: true, ip: "10.42.0.9" });
      },
    });
    const { result } = renderHook(() => useAttackBox("ab-keep", { running: true }));
    await flush();
    fail = true;
    act(() => {
      vi.advanceTimersByTime(5000);
    });
    await flush();
    expect(result.current.status?.running).toBe(true);
  });
});

describe("attachAttackBox", () => {
  it("joins an attach already under way instead of running a second one", async () => {
    let finish!: () => void;
    const calls = installTauri({
      attack_box_attach: () =>
        new Promise<null>(
          (r) =>
            (finish = () => {
              r(null);
            }),
        ),
    });
    const a = attachAttackBox("ab-join", "DOCKER", null);
    const b = attachAttackBox("ab-join", "DOCKER", null);
    await flush();
    expect(count(calls, "attack_box_attach")).toBe(1);
    finish();
    await Promise.all([a, b]);
    // Done: the next attach runs again.
    void attachAttackBox("ab-join", "DOCKER", null);
    await flush();
    expect(count(calls, "attack_box_attach")).toBe(2);
    finish();
  });
});

describe("the attack box's attach", () => {
  it("is joined by the lab page's Start while the auto-start with the lab adds it", async () => {
    let finish!: () => void;
    const calls = installTauri({
      attack_box_status: () => box({ running: false }),
      attack_box_attach: () =>
        new Promise<null>(
          (r) =>
            (finish = () => {
              r(null);
            }),
        ),
    });
    // The app-wide auto-start adds the box; the page, opened meanwhile, still reads it down.
    const auto = attachAttackBox("ab-vm-twice", "VM", null);
    const page = renderHook(() => useAttackBox("ab-vm-twice", { running: true, runtime: "VM" }));
    await flush();
    expect(count(calls, "attack_box_attach")).toBe(1);
    // A click joins the attach under way.
    await act(async () => {
      void page.result.current.start();
      await flush();
    });
    expect(count(calls, "attack_box_attach")).toBe(1);
    await act(async () => {
      finish();
      await auto;
      await flush();
    });
  });
});

describe("useAttackBoxAutoStart", () => {
  /** An operation as a worker reports it, in `runtime` (null: one in this process, which says none). */
  const op = (labId: string, o: ActiveOperation["op"], runtime: Runtime | null = "DOCKER"): [string, ActiveOperation] => [
    labId,
    { labId, op: o, machine: null, step: null, runtime },
  ];

  function watch(calls: Call[], enabled = true, launched: Runtime | null = "DOCKER") {
    const hook = renderHook(
      (p: { ops: Map<string, ActiveOperation> }) => {
        useAttackBoxAutoStart(p.ops, enabled);
      },
      {
        initialProps: { ops: new Map([op("lab-a", "launch", launched), op("lab-b", "stop")]) },
      },
    );
    return { hook, calls };
  }

  it("attaches the box of a container lab whose launch just ended", async () => {
    const calls = installTauri({
      lab_status: () => ({ running: true, place: "container" }),
      attack_box_status: () => box(),
    });
    const { hook } = watch(calls);
    hook.rerender({ ops: new Map() });
    await flush(50);
    expect(calls.find((c) => c.cmd === "lab_status")?.args).toEqual({ id: "lab-a", runtime: "DOCKER" });
    // Only the launch, not the stop.
    expect(calls.filter((c) => c.cmd === "lab_status")).toHaveLength(1);
    expect(calls.find((c) => c.cmd === "attack_box_attach")?.args).toMatchObject({ id: "lab-a" });
  });

  it("leaves a box already running, a failed launch and a box the backend can't reach (an ESXi host) alone", async () => {
    for (const [status, running, reachable] of [
      [{ running: true, place: "container" }, true, true],
      [{ running: false, place: null }, false, true],
      [{ running: true, place: "server", provider: "vmware_esxi" }, false, false],
    ] as const) {
      const calls = installTauri({ lab_status: () => status, attack_box_status: () => box({ running, reachable }) });
      const { hook } = watch(calls);
      hook.rerender({ ops: new Map() });
      await flush(50);
      expect(commands(calls)).not.toContain("attack_box_attach");
      hook.unmount();
    }
  });

  it("attaches the box wherever the lab runs, before the box exists", async () => {
    for (const place of ["cloud", "server", "local_vm"] as const) {
      const calls = installTauri({ lab_status: () => ({ running: true, place, provider: "proxmox" }), attack_box_status: () => box() });
      const { hook } = watch(calls);
      hook.rerender({ ops: new Map() });
      await flush(50);
      expect(commands(calls), place).toContain("attack_box_attach");
      hook.unmount();
    }
  });

  it("reads and attaches in the runtime the catalogue declares for the lab, never a cloud-services lab's", async () => {
    for (const [runtime, attached] of [
      ["VM", true],
      ["CLOUD", false],
    ] as const) {
      setCatalogue("auto-rt", [{ id: "lab-a", runtime: { runtime } } as Lab]);
      // The backend reaches no run to put a box in beside a cloud-services lab.
      const calls = installTauri({ lab_status: () => ({ running: true, place: "local_vm" }), attack_box_status: () => box({ vm: true, reachable: attached }) });
      // An operation in this process says no runtime: the catalogue's stands in.
      const { hook } = watch(calls, true, null);
      hook.rerender({ ops: new Map() });
      await flush(50);
      expect(commands(calls).includes("attack_box_attach")).toBe(attached);
      if (attached) {
        expect(calls.find((c) => c.cmd === "lab_status")?.args).toEqual({ id: "lab-a", runtime: "VM" });
        expect(calls.find((c) => c.cmd === "attack_box_attach")?.args).toMatchObject({ id: "lab-a", runtime: "VM" });
      }
      hook.unmount();
    }
    setCatalogue("auto-rt", []);
  });

  it("reads the lab in the runtime its worker ran it in, and guesses none when nothing says", async () => {
    const calls = installTauri({ lab_status: () => ({ running: true, place: "local_vm" }), attack_box_status: () => box({ vm: true }) });
    const { hook } = watch(calls, true, "VM");
    hook.rerender({ ops: new Map() });
    await flush(50);
    expect(calls.find((c) => c.cmd === "attack_box_attach")?.args).toMatchObject({ id: "lab-a", runtime: "VM" });
    hook.unmount();
    // Not in the catalogue, and an operation in this process: nothing is read nor attached.
    const unknown = installTauri({ lab_status: () => ({ running: true, place: "container" }), attack_box_status: () => box() });
    const second = watch(unknown, true, null);
    second.hook.rerender({ ops: new Map() });
    await flush(50);
    expect(commands(unknown)).not.toContain("lab_status");
    expect(commands(unknown)).not.toContain("attack_box_attach");
    second.hook.unmount();
  });

  it("tells the player when the box couldn't be added with the lab", async () => {
    vi.spyOn(console, "warn").mockImplementation(() => {});
    vi.spyOn(document, "hasFocus").mockReturnValue(true);
    render(<Toaster />);
    const calls = installTauri({
      lab_status: () => ({ running: true, place: "container" }),
      attack_box_status: () => box(),
      attack_box_attach: () => rejectWith("image pull failed"),
    });
    const { hook } = watch(calls);
    hook.rerender({ ops: new Map() });
    await flush(50);
    expect(document.body.textContent).toContain("The attack box couldn't be added with the lab");
    expect(document.body.textContent).toContain("image pull failed");
    vi.restoreAllMocks();
  });

  it("does nothing when disabled or when Settings turn auto-start off", async () => {
    const calls = installTauri({ lab_status: () => ({ running: true, place: "container" }), attack_box_status: () => box() });
    const { hook } = watch(calls, false);
    hook.rerender({ ops: new Map() });
    await flush(50);
    setAutoAttackBox(false);
    const second = watch(calls);
    second.hook.rerender({ ops: new Map() });
    await flush(50);
    expect(commands(calls)).not.toContain("lab_status");
  });
});

describe("AttackBoxPanel", () => {
  const fake = (p: Partial<AttackBox> = {}): AttackBox => ({
    status: null,
    busy: false,
    log: [],
    start: vi.fn(async () => {}),
    stop: vi.fn(async () => {}),
    vm: false,
    name: "nwodtuhs/exegol:free",
    networks: [],
    network: null,
    defaultNetwork: "",
    setNetwork: vi.fn(),
    ...p,
  });

  it("offers to start a box that isn't running", async () => {
    const user = userEvent.setup();
    const b = fake({ status: box() });
    render(<AttackBoxPanel box={b} running host={null} />);
    expect(screen.getByText("not started")).toBeTruthy();
    expect(screen.getByText(/a container among the lab's, on the lab network/)).toBeTruthy();
    await user.click(screen.getByRole("button", { name: /Start attack box/ }));
    expect(b.start).toHaveBeenCalledTimes(1);
  });

  it("lets the learner pick which lab network the box joins when there is more than one, the backend's default shown first", async () => {
    const user = userEvent.setup();
    const b = fake({ status: box({ networks: ["dmz", "internal"] }), networks: ["dmz", "internal"], defaultNetwork: "internal" });
    render(<AttackBoxPanel box={b} running host={null} />);
    expect(screen.getByRole<HTMLSelectElement>("combobox").value).toBe("internal");
    await user.selectOptions(screen.getByRole("combobox"), "dmz");
    expect(b.setNetwork).toHaveBeenCalledWith("dmz");
  });

  it("offers no pick where the lab declares the player's seat", () => {
    const b = fake({ status: box({ networks: [], defaultNetwork: "access" }), networks: [], defaultNetwork: "access" });
    render(<AttackBoxPanel box={b} running host={null} />);
    expect(screen.queryByRole("combobox")).toBeNull();
  });

  it("shows a running box with its address, a shell and Stop", async () => {
    const user = userEvent.setup();
    const onShell = vi.fn();
    const b = fake({ status: box({ running: true, ip: "10.42.0.9" }) });
    render(<AttackBoxPanel box={b} running host={null} onShell={onShell} shellReady />);
    expect(screen.getByText("running")).toBeTruthy();
    expect(screen.getByText("10.42.0.9")).toBeTruthy();
    await user.click(screen.getByRole("button", { name: /Open attacker shell/ }));
    expect(onShell).toHaveBeenCalled();
    await user.click(screen.getByRole("button", { name: /Stop attack box/ }));
    expect(b.stop).toHaveBeenCalled();
  });

  it("says the box starts with the lab before the lab runs, with no controls", () => {
    render(<AttackBoxPanel box={fake()} running={false} host={null} />);
    expect(screen.getByText("starts with the lab")).toBeTruthy();
    expect(screen.queryByRole("button")).toBeNull();
  });

  it("shows a box working and a failed start's log", () => {
    render(
      <AttackBoxPanel
        box={fake({ busy: true, status: box({ running: true }), log: ["Removing the attack box…", "✗ docker: no such container"] })}
        running
        host={null}
      />,
    );
    expect(screen.getByRole("button", { name: /Working…/ }).hasAttribute("disabled")).toBe(true);
    expect(screen.getAllByText(/no such container/).length).toBeGreaterThan(0);
  });

  it("says starting while an attach runs", () => {
    render(<AttackBoxPanel box={fake({ busy: true })} running host={null} />);
    expect(screen.getByText("starting")).toBeTruthy();
  });

  it("shows a remote lab's box as the runner reports it, started and stopped as here", async () => {
    const user = userEvent.setup();
    const down = fake({ status: box() });
    const { unmount } = render(<AttackBoxPanel box={down} running host="homelab-pve" onShell={vi.fn()} shellReady={false} />);
    // Not "running on" before the box is: the lab running says nothing of its box.
    expect(screen.queryByText("running on homelab-pve")).toBeNull();
    expect(screen.getByText("not started")).toBeTruthy();
    expect(screen.getByText(/a container among the lab's, on the lab network/)).toBeTruthy();
    expect(screen.queryByRole("button", { name: /Open attacker shell/ })).toBeNull();
    await user.click(screen.getByRole("button", { name: /Start attack box/ }));
    expect(down.start).toHaveBeenCalledTimes(1);
    unmount();
    const up = fake({ status: box({ running: true, ip: "10.30.0.77" }) });
    render(<AttackBoxPanel box={up} running host="homelab-pve" onShell={vi.fn()} shellReady />);
    expect(screen.getByText("running on homelab-pve")).toBeTruthy();
    expect(screen.getByText("10.30.0.77")).toBeTruthy();
    expect(screen.getByRole("button", { name: /Open attacker shell/ })).toBeTruthy();
    await user.click(screen.getByRole("button", { name: /Stop attack box/ }));
    expect(up.stop).toHaveBeenCalledTimes(1);
  });

  it("names the attack VM beside a VM lab", () => {
    render(<AttackBoxPanel box={fake({ vm: true, name: "kalilinux/rolling" })} running host={null} />);
    expect(screen.getByText("Attack VM")).toBeTruthy();
    expect(screen.getByText("kalilinux/rolling")).toBeTruthy();
    expect(screen.getByText(/Your Kali machine in the lab: a VM among the lab's/)).toBeTruthy();
  });

  it("offers to start the attack VM beside a VM lab on any hypervisor, QEMU included", () => {
    render(<AttackBoxPanel box={fake({ vm: true, name: "this machine's default box", status: box() })} running host={null} />);
    expect(screen.getByText("Attack VM")).toBeTruthy();
    expect(screen.getByText("this machine's default box")).toBeTruthy();
    expect(screen.getByRole("button", { name: /Start attack box/ })).toBeTruthy();
  });
});

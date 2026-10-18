// @vitest-environment jsdom
import { act, render, renderHook, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";
import { AttackBoxPanel } from "@/features/labs/attack-box-panel";
import { attachContainerBox, useAttackBoxAutoStart } from "@/features/labs/attack-box-auto";
import { useAttackBox, type AttackBox } from "@/features/labs/use-attack-box";
import type { ActiveOperation, AttackStatus } from "@/lib/tauri";
import { setAttackBox, setAttackImage, setAutoAttackBox } from "@/lib/settings";
import { commands, installTauri, stream, type Call } from "@/test/tauri";
import { flush } from "@/test/ui";
import { rejectWith } from "@/features/labs/labs-test-kit";

const box = (p: Partial<AttackStatus> = {}): AttackStatus => ({
  running: false,
  ip: "",
  labNetwork: "default",
  networks: ["default"],
  shellCmd: "docker compose exec kali bash",
  ...p,
});
const count = (calls: Call[], cmd: string) => calls.filter((c) => c.cmd === cmd).length;

afterEach(() => {
  vi.useRealTimers();
  setAutoAttackBox(true);
});

describe("useAttackBox", () => {
  it("reads the container attack box for the image in Settings and polls it while the lab runs", async () => {
    vi.useFakeTimers({ toFake: ["setInterval", "clearInterval"] });
    setAttackImage("my/exegol:full");
    const calls = installTauri({ attack_box_status: () => box({ running: true, ip: "10.42.0.9" }) });
    const { result, rerender } = renderHook((p: { running: boolean }) => useAttackBox("ab-poll", { running: p.running, local: true }), {
      initialProps: { running: true },
    });
    await flush();
    expect(calls.find((c) => c.cmd === "attack_box_status")?.args).toEqual({ id: "ab-poll", runtime: "DOCKER", network: null });
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

  it("never reads the box of a lab on a server", async () => {
    const calls = installTauri();
    renderHook(() => useAttackBox("ab-remote", { running: true, local: false }));
    await flush();
    expect(count(calls, "attack_box_status")).toBe(0);
  });

  it("reads and attaches the box beside a VM lab with the box from Settings", async () => {
    setAttackBox("kalilinux/rolling");
    setAutoAttackBox(false);
    const calls = installTauri({
      attack_box_status: () => box(),
      attack_box_attach: (a) => {
        stream(a, "==> attach: adding kali");
        return null;
      },
    });
    const { result } = renderHook(() => useAttackBox("ab-vm", { running: true, local: true, runtime: "VM" }));
    await flush();
    expect(calls.find((c) => c.cmd === "attack_box_status")?.args).toEqual({ id: "ab-vm", runtime: "VM", network: null });
    expect(result.current.name).toBe("kalilinux/rolling");
    await act(() => result.current.start());
    expect(calls.find((c) => c.cmd === "attack_box_attach")?.args).toMatchObject({ id: "ab-vm", runtime: "VM", pick: { image: "kalilinux/rolling" } });
    expect(result.current.log).toEqual(["Starting the attack box…", "==> attach: adding kali"]);
    await act(() => result.current.stop());
    expect(calls.find((c) => c.cmd === "attack_box_detach")?.args).toMatchObject({ id: "ab-vm", runtime: "VM" });
  });

  it("attaches the box with the lab once, when Settings say so", async () => {
    const calls = installTauri({ attack_box_status: () => box() });
    renderHook(() => useAttackBox("ab-auto", { running: true, local: true }));
    await flush(50);
    expect(count(calls, "attack_box_attach")).toBe(1);
    expect(calls.find((c) => c.cmd === "attack_box_attach")?.args).toMatchObject({ id: "ab-auto" });
  });

  it("doesn't attach the box beside a lab whose deploy failed, or with auto-start off", async () => {
    const calls = installTauri({ attack_box_status: () => box() });
    renderHook(() => useAttackBox("ab-failed", { running: true, local: true, failed: true }));
    await flush(50);
    setAutoAttackBox(false);
    renderHook(() => useAttackBox("ab-off", { running: true, local: true }));
    await flush(50);
    expect(count(calls, "attack_box_attach")).toBe(0);
  });

  it("doesn't touch the box while a lab operation is in flight", async () => {
    const calls = installTauri({ attack_box_status: () => box() });
    renderHook(() => useAttackBox("ab-hold", { running: true, holding: true, local: true }));
    await flush(50);
    expect(count(calls, "attack_box_status")).toBe(0);
    expect(count(calls, "attack_box_attach")).toBe(0);
  });

  it("shows a detached box at once and puts a failure on the log", async () => {
    setAutoAttackBox(false);
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
    const { result } = renderHook(() => useAttackBox("ab-stop", { running: true, local: true }));
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
    vi.useFakeTimers({ toFake: ["setInterval", "clearInterval"] });
    installTauri({
      attack_box_status: () => {
        if (fail) throw new Error("runner busy");
        return box({ running: true, ip: "10.42.0.9" });
      },
    });
    const { result } = renderHook(() => useAttackBox("ab-keep", { running: true, local: true }));
    await flush();
    fail = true;
    act(() => {
      vi.advanceTimersByTime(5000);
    });
    await flush();
    expect(result.current.status?.running).toBe(true);
  });
});

describe("attachContainerBox", () => {
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
    const a = attachContainerBox("ab-join");
    const b = attachContainerBox("ab-join");
    await flush();
    expect(count(calls, "attack_box_attach")).toBe(1);
    finish();
    await Promise.all([a, b]);
    // Done: the next attach runs again.
    void attachContainerBox("ab-join");
    await flush();
    expect(count(calls, "attack_box_attach")).toBe(2);
    finish();
  });
});

describe("the attack box's attach", () => {
  it("runs once when its lab page is left and opened again while it attaches", async () => {
    setAutoAttackBox(true);
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
    const visit = () => renderHook(() => useAttackBox("ab-vm-twice", { running: true, local: true, runtime: "VM" }));
    const first = visit();
    await flush();
    expect(count(calls, "attack_box_attach")).toBe(1);
    // Left while the attach runs; back again: the status still reads "not running".
    first.unmount();
    const second = visit();
    await flush();
    expect(count(calls, "attack_box_attach")).toBe(1);
    // A click joins the attach under way too.
    await act(async () => {
      void second.result.current.start();
      await flush();
    });
    expect(count(calls, "attack_box_attach")).toBe(1);
    await act(async () => {
      finish();
      await flush();
    });
  });
});

describe("useAttackBoxAutoStart", () => {
  const op = (labId: string, o: ActiveOperation["op"]): [string, ActiveOperation] => [labId, { labId, op: o, machine: null, step: null }];

  function watch(calls: Call[], enabled = true) {
    const hook = renderHook(
      (p: { ops: Map<string, ActiveOperation> }) => {
        useAttackBoxAutoStart(p.ops, enabled);
      },
      {
        initialProps: { ops: new Map([op("lab-a", "launch"), op("lab-b", "stop")]) },
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

  it("leaves a box already running, a failed launch and a lab in a VM alone", async () => {
    for (const [status, running] of [
      [{ running: true, place: "container" }, true],
      [{ running: false, place: null }, false],
      [{ running: true, place: "local_vm" }, false],
    ] as const) {
      const calls = installTauri({ lab_status: () => status, attack_box_status: () => box({ running }) });
      const { hook } = watch(calls);
      hook.rerender({ ops: new Map() });
      await flush(50);
      expect(commands(calls)).not.toContain("attack_box_attach");
      hook.unmount();
    }
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
    setNetwork: vi.fn(),
    ...p,
  });

  it("offers to start a box that isn't running", async () => {
    const user = userEvent.setup();
    const b = fake({ status: box() });
    render(<AttackBoxPanel box={b} running host={null} />);
    expect(screen.getByText("not started")).toBeTruthy();
    expect(screen.getByText("Your machine on the lab network. Open its shell to attack the targets from inside the lab.")).toBeTruthy();
    await user.click(screen.getByRole("button", { name: /Start attack box/ }));
    expect(b.start).toHaveBeenCalledTimes(1);
  });

  it("lets the learner pick which lab network the box joins when there is more than one", async () => {
    const user = userEvent.setup();
    const b = fake({ status: box({ networks: ["dmz", "internal"] }), networks: ["dmz", "internal"] });
    render(<AttackBoxPanel box={b} running host={null} />);
    await user.selectOptions(screen.getByRole("combobox"), "internal");
    expect(b.setNetwork).toHaveBeenCalledWith("internal");
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

  it("shows a remote lab's box as running on its host, without start or stop", () => {
    render(<AttackBoxPanel box={fake()} running host="homelab-pve" />);
    expect(screen.getByText("running on homelab-pve")).toBeTruthy();
    expect(screen.getByText(/connects over SSH/)).toBeTruthy();
    expect(screen.queryByRole("button", { name: /attack box/ })).toBeNull();
  });

  it("names the attack VM beside a VM lab", () => {
    render(<AttackBoxPanel box={fake({ vm: true, name: "kalilinux/rolling" })} running host={null} />);
    expect(screen.getByText("Attack VM")).toBeTruthy();
    expect(screen.getByText("kalilinux/rolling")).toBeTruthy();
    expect(screen.getByText(/Your own VM beside the lab/)).toBeTruthy();
  });

  it("shows the QEMU controller as the attacker, with nothing to start", () => {
    render(<AttackBoxPanel box={fake({ vm: true, status: box() })} running host={null} controller />);
    expect(screen.getByText("Attacker")).toBeTruthy();
    expect(screen.getByText("isoloom-controller")).toBeTruthy();
    expect(screen.queryByRole("button")).toBeNull();
  });
});

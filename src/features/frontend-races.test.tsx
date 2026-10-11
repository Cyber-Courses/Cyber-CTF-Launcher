// @vitest-environment jsdom
import { act, render, renderHook, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { emit } from "@tauri-apps/api/event";
import { afterEach, describe, expect, it, vi } from "vitest";
import { CONFIRM_WINDOW_MS, LaunchConfirm } from "@/features/app/launch-confirm";
import { useHostedLabs, type HostedSession } from "@/features/hosted/use-hosted-labs";
import { LabDetail } from "@/features/labs/lab-detail";
import { useLabCheck } from "@/features/labs/lab-health";
import { getLabState, noteStopped, refreshStatus, setScanRunning } from "@/features/labs/lab-store";
import { useAttackBox } from "@/features/labs/use-attack-box";
import { useLabActions } from "@/features/labs/use-lab-actions";
import { useLabs, type Lab } from "@/features/labs/use-labs";
import { beginDeploy, appendDeployLog, endDeploy, getDeploySnapshot } from "@/lib/deploy-store";
import { setLocalePreference, translate } from "@/lib/i18n";
import type { LabCheck, LabStatus } from "@/lib/tauri";
import { commands, installTauri } from "@/test/tauri";
import { button, flush } from "@/test/ui";

// Regression tests for polling and state races on the frontend: each failed before its fix.

const lab = (id: string, runtime: "DOCKER" | "VM" = "DOCKER"): Lab => ({
  id,
  slug: id,
  title: `Lab ${id}`,
  description: null,
  question: null,
  difficulty: 1,
  category: "Web",
  runtime: { runtime, architectures: ["x86_64"], providers: ["virtualbox"] },
  skills: [],
});

const status = (p: Partial<LabStatus> = {}): LabStatus => ({
  running: true,
  parked: null,
  machines: [],
  networks: [],
  url: null,
  host: null,
  expiresAt: null,
  place: "container",
  provider: "docker",
  attacker: null,
  outputs: [],
  message: null,
  ...p,
});

/** A promise and the functions settling it, for a reply the test decides when to give. */
function deferred<T>() {
  let resolve!: (v: T) => void;
  let reject!: (e: unknown) => void;
  const promise = new Promise<T>((res, rej) => {
    resolve = res;
    reject = rej;
  });
  return { promise, resolve, reject };
}

/** Advances fake time and lets the promises it releases settle. */
const advance = (ms: number) =>
  act(async () => {
    await vi.advanceTimersByTimeAsync(ms);
  });

afterEach(() => {
  vi.useRealTimers();
});

describe("hosted lab poll", () => {
  const session: HostedSession = {
    id: "sess-1",
    labId: "lab-h",
    target: "hosted",
    state: "REQUESTED",
    endpoints: [],
    message: null,
    expiresAt: "2030-01-01T00:00:00Z",
  };

  it("keeps polling after a failed read and a null answer", async () => {
    vi.useFakeTimers();
    const replies: (() => unknown)[] = [
      () => {
        throw new Error("network down");
      },
      () => ({ labSession: null }),
      () => ({ labSession: { ...session, state: "RUNNING", endpoints: [{ port: 80, url: "https://lab.example" }] } }),
    ];
    installTauri({
      api_query: (a) =>
        String(a?.operation).includes("activeSession") ? { myActiveLabSession: session } : (replies.shift() ?? (() => ({ labSession: null })))(),
    });
    const { result } = renderHook(() => useHostedLabs());
    await advance(0);
    expect(result.current.session?.state).toBe("REQUESTED");
    await advance(2500);
    await advance(2500);
    await advance(2500);
    expect(result.current.session?.state).toBe("RUNNING");
  });

  it("gives up with a message once the start deadline passes, even when every read fails", async () => {
    vi.useFakeTimers();
    setLocalePreference("en");
    installTauri({
      api_query: (a) => {
        if (String(a?.operation).includes("activeSession")) return { myActiveLabSession: session };
        throw new Error("backend unreachable");
      },
    });
    const { result } = renderHook(() => useHostedLabs());
    await advance(0);
    for (let i = 0; i < 245; i++) await advance(2500);
    expect(result.current.error).toBe(translate("hosted.slowStart"));
    setLocalePreference("system");
  });
});

describe("lab health check", () => {
  it("checks again when another container goes down mid-check, instead of spinning", async () => {
    const replies: ReturnType<typeof deferred<LabCheck>>[] = [];
    installTauri({
      lab_check: () => {
        const d = deferred<LabCheck>();
        replies.push(d);
        return d.promise;
      },
    });
    const { result, rerender } = renderHook((p: { downCount: number }) => useLabCheck("lab-c", { running: true, downCount: p.downCount, enabled: true }), {
      initialProps: { downCount: 1 },
    });
    await flush();
    expect(result.current[0]).toBe("checking");
    rerender({ downCount: 2 });
    await flush();
    for (const r of replies) r.resolve({ available: true, ok: true, output: "", results: [] });
    await flush();
    expect(result.current[0]).toMatchObject({ ok: true });
  });
});

describe("lab page actions during a worker operation", () => {
  const detail = (id: string) =>
    render(
      <LabDetail
        lab={lab(id)}
        status={status({ running: true })}
        busy={false}
        logs={[]}
        times={[]}
        loggedIn
        hostArch="x86_64"
        dockerRunning
        onBack={vi.fn()}
        onStart={vi.fn()}
        onStop={vi.fn()}
        onPark={vi.fn()}
        onResume={vi.fn()}
      />,
    );

  it("disables Stop & remove while the backend reports an operation in flight (after a reload)", async () => {
    installTauri({
      lab_status: () => status(),
      deploying_labs: () => ["lab-w"],
      active_operations: () => [{ labId: "lab-w", op: "provision", machine: null, step: null }],
    });
    detail("lab-w");
    await flush(50);
    expect(button(/Stop & remove/).hasAttribute("disabled")).toBe(true);
  });

  it("a refused operation keeps the previous run's log and isn't a failed deploy", async () => {
    installTauri({
      lab_park: () => {
        throw "Pause of this lab is already in progress; wait for it to finish.";
      },
      lab_status: () => status(),
    });
    beginDeploy("lab-r", "launch");
    appendDeployLog("lab-r", "✓ Lab is running");
    endDeploy("lab-r");
    const { result } = renderHook(() => useLabActions(() => {}));
    await act(async () => {
      await result.current.park(lab("lab-r"), "pause");
    });
    expect(getDeploySnapshot().runs["lab-r"].logs).toEqual(["✓ Lab is running"]);
    expect(getDeploySnapshot().runs["lab-r"].busy).toBe(false);
  });
});

describe("reset", () => {
  it("doesn't start the lab again when the stop failed", async () => {
    installTauri({ lab_status: () => status() });
    const user = userEvent.setup();
    const onStart = vi.fn();
    const onStop = vi.fn(async () => "vagrant destroy failed");
    render(
      <LabDetail
        lab={lab("lab-s")}
        status={status({
          machines: [
            { name: "api", state: "running", image: "x", ip: "10.0.0.2", ports: [], interfaces: [], services: [], infra: false },
            { name: "db", state: "exited", image: "x", ip: "", ports: [], interfaces: [], services: [], infra: false },
          ],
        })}
        busy={false}
        logs={[]}
        times={[]}
        loggedIn
        hostArch="x86_64"
        dockerRunning
        onBack={vi.fn()}
        onStart={onStart}
        onStop={onStop}
      />,
    );
    await flush();
    await user.click(button(/Reset lab/));
    await flush();
    expect(onStop).toHaveBeenCalled();
    expect(onStart).not.toHaveBeenCalled();
  });

  it("stop resolves to its failure", async () => {
    installTauri({
      lab_stop: () => {
        throw "vagrant destroy failed";
      },
      lab_status: () => status(),
    });
    const { result } = renderHook(() => useLabActions(() => {}));
    let failure: string | null = null;
    await act(async () => {
      failure = await result.current.stop(lab("lab-f"));
    });
    expect(failure).toContain("vagrant destroy failed");
  });
});

describe("cloud launch confirmation", () => {
  it("expires with the backend's window instead of letting a late Approve do nothing", async () => {
    vi.useFakeTimers({ toFake: ["setTimeout", "clearTimeout"] });
    const calls = installTauri();
    render(<LaunchConfirm />);
    await act(async () => {
      await emit("launch-confirm", { sessionId: "s-1", repository: "org/lab", commit: "abcdef1234567890", target: "aws" });
      await vi.advanceTimersByTimeAsync(0);
    });
    expect(screen.getByRole("alertdialog")).toBeTruthy();
    await advance(CONFIRM_WINDOW_MS);
    expect(screen.queryByText(translate("app.launchConfirm.approve"))).toBeNull();
    expect(screen.getByText(translate("app.launchConfirm.expired"))).toBeTruthy();
    await act(async () => {
      button(new RegExp(translate("app.launchConfirm.close"))).click();
    });
    expect(screen.queryByRole("alertdialog")).toBeNull();
    expect(commands(calls)).not.toContain("confirm_launch");
  });
});

describe("attack box poll", () => {
  it("doesn't stack status reads while one is in flight", async () => {
    vi.useFakeTimers();
    const calls = installTauri({ attack_vm_status: () => new Promise(() => {}) });
    renderHook(() => useAttackBox("lab-a", { running: true, local: true, kind: "vm" }));
    await advance(0);
    await advance(5000 * 4);
    expect(commands(calls).filter((c) => c === "attack_vm_status")).toHaveLength(1);
  });
});

describe("a stale scan after a local stop", () => {
  it("doesn't bring a stopped lab back to running until a read confirms it", async () => {
    installTauri({ lab_status: () => status({ running: true }) });
    setScanRunning(["lab-d"], Date.now() - 1000);
    noteStopped("lab-d");
    expect(getLabState().scanRunning.has("lab-d")).toBe(false);
    // The backend's cached answer from before the stop, to a scan asked after it.
    expect(setScanRunning(["lab-d"], Date.now() + 1)).toEqual(["lab-d"]);
    expect(getLabState().scanRunning.has("lab-d")).toBe(false);
    // A status read after the stop sees it running (started from the website): the scan counts again.
    await refreshStatus(lab("lab-d"));
    setScanRunning(["lab-d"], Date.now() + 2);
    expect(getLabState().scanRunning.has("lab-d")).toBe(true);
  });

  it("shows a lab stopped here as stopped while the scan still reports it, so no attack box starts", async () => {
    const stale = lab("lab-e");
    const calls = installTauri({
      api_query: (a) => (String(a?.operation).includes("completedLabs") ? { myCompletedLabs: [] } : { labs: [stale] }),
      lab_status: () => status({ running: false, parked: null }),
      running_labs: () => ["lab-e"],
    });
    noteStopped("lab-e");
    const { result } = renderHook(() => useLabs("stale-scan"));
    await flush(50);
    expect(commands(calls)).toContain("running_labs");
    expect(result.current.statuses["lab-e"]?.running).toBe(false);
  });
});

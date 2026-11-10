import { describe, expect, it } from "vitest";

import {
  bindHost,
  deployPlacement,
  diagramAbove,
  headerMode,
  networkView,
  removing,
  publishedBinds,
  restartTarget,
} from "@/features/labs/lab-detail/lab-state";
import type { LabRuntimeInfo } from "@/features/labs/use-labs";
import type { RunTarget } from "@/features/labs/run-target";
import type { LabControl, Provider, ServerHost } from "@/lib/tauri";
import { deriveState as derive, labMachine as machine, labStatus as status } from "@/features/labs/labs-test-kit";

const docker: LabRuntimeInfo = { runtime: "DOCKER", architectures: [], providers: [] };
const vm: LabRuntimeInfo = { runtime: "VM", architectures: ["x86_64"], providers: ["virtualbox"] as Provider[] };

describe("deriveLabState", () => {
  it("reads a lab never started as not started", () => {
    const s = derive({ status: status({}) });
    expect(s).toMatchObject({ running: false, starting: false, parked: null, interrupted: false, operation: null, phase: "notStarted" });
  });

  it("does not count machines Vagrant has not created as leftovers", () => {
    expect(derive({ runtime: vm, status: status({ machines: [machine("dc01", "not_created")] }) }).interrupted).toBe(false);
    expect(derive({ runtime: vm, status: status({ machines: [machine("dc01", "poweroff")] }) }).phase).toBe("interrupted");
  });

  it("starts while this session's deploy runs, with a launch in flight", () => {
    const s = derive({ busy: true, status: status({}) });
    expect(s).toMatchObject({ deploying: true, starting: true, operation: "launch", phase: "operation" });
  });

  it("recovers a deploy the backend still runs after a reload", () => {
    const s = derive({ backendDeploying: true, backendOp: "resume", status: status({}) });
    expect(s).toMatchObject({ starting: true, operation: "resume" });
  });

  it("reads a busy built lab as being stopped, unless the page or backend says otherwise", () => {
    expect(derive({ busy: true, status: status({ running: true }) }).operation).toBe("stop");
    expect(derive({ busy: true, status: status({ parked: "pause" }) }).operation).toBe("stop");
    expect(derive({ busy: true, acting: "pause", backendOp: "stop", status: status({ running: true }) }).operation).toBe("pause");
  });

  it("tears down on stop, shut down and pause only", () => {
    expect(derive({ busy: true, acting: "shutdown", status: status({ running: true }) }).tearingDown).toBe(true);
    expect(derive({ busy: true, acting: "provision", status: status({ running: true }) }).tearingDown).toBe(false);
  });

  it("keeps the running word while an operation without its own (provision) runs", () => {
    expect(derive({ busy: true, acting: "provision", status: status({ running: true }) }).phase).toBe("running");
  });

  it("is parked only when nothing is in flight", () => {
    expect(derive({ status: status({ parked: "shutdown" }) })).toMatchObject({ parked: "shutdown", phase: "shutDown" });
    expect(derive({ status: status({ parked: "pause" }) }).phase).toBe("paused");
    expect(derive({ busy: true, status: status({ parked: "pause" }) }).parked).toBeNull();
  });

  it("lists machines down while running, but not the controller nor during a deploy", () => {
    const machines = [machine("web"), machine("db", "exited"), machine("isoloom-controller", "poweroff", { infra: true })];
    expect(derive({ status: status({ running: true, machines }) }).down.map((m) => m.name)).toEqual(["db"]);
    expect(derive({ busy: true, status: status({ running: true, machines }) }).down).toEqual([]);
  });

  it("offers what the backend says the lifecycle accepts where the lab runs, with the page's handlers", () => {
    // A container lab run as one VM per machine: catalogue DOCKER, a VM lab where it runs (its
    // down machines brought back as they are, like a VM lab's).
    const asVms = derive({
      runtime: docker,
      status: status({ running: true, place: "local_vm", perMachine: true, controls: ["pause", "shutdown", "provision", "repair"] }),
    });
    expect([asVms.canPause, asVms.canShutdown, asVms.canProvision, asVms.canRepair]).toEqual([true, true, true, true]);
    const inVm = derive({ status: status({ running: true, place: "local_vm", controls: ["pause"] }) });
    expect([inVm.canPause, inVm.canShutdown, inVm.canProvision, inVm.canRepair]).toEqual([true, false, false, false]);
    // Nothing the backend doesn't offer, whatever the catalogue says.
    const remote = derive({ runtime: vm, status: status({ running: true, host: "pve", hostId: "pve", place: "server", provider: "proxmox" }) });
    expect([remote.canPause, remote.canShutdown, remote.canProvision, remote.canRepair]).toEqual([false, false, false, false]);
    // Nor without the page's handlers.
    const all = status({ running: true, controls: ["pause", "shutdown", "provision", "repair"] });
    const unwired = derive({ parkable: false, provisionable: false, status: all });
    expect([unwired.canPause, unwired.canShutdown, unwired.canProvision]).toEqual([false, false, false]);
  });

  it("is native unless the lab is built for another CPU, and emulates with QEMU ready", () => {
    expect(derive({ runtime: vm, hostArch: "arm64" }).native).toBe(false);
    expect(derive({ runtime: vm, hostArch: "" }).native).toBe(true);
    expect(derive({ readyVms: ["qemu"] as Provider[] }).emulates).toBe(true);
  });
});

describe("headerMode", () => {
  const opts = { loggedIn: true, canLogin: true, canResume: true };
  it("follows what the lab is doing", () => {
    expect(headerMode(derive({ status: status({ running: true }) }), opts)).toBe("running");
    expect(headerMode(derive({ status: status({ parked: "pause" }) }), opts)).toBe("parked");
    expect(headerMode(derive({ busy: true, status: status({}) }), opts)).toBe("starting");
    expect(headerMode(derive({ status: status({ machines: [machine("web", "exited")] }) }), opts)).toBe("interrupted");
    expect(headerMode(derive({ status: status({}) }), opts)).toBe("start");
  });
  it("reads cleaning up an interrupted run as a stop", () => {
    expect(headerMode(derive({ busy: true, backendOp: "stop", status: status({}) }), opts)).toBe("stopping");
  });
  it("offers Retry after a failed start, the clean-up first when it left machines", () => {
    const failed = derive({ startFailed: true, status: status({}) });
    expect(failed.phase).toBe("failed");
    expect(headerMode(failed, opts)).toBe("failed");
    // Signed out meanwhile: sign in first.
    expect(headerMode(failed, { ...opts, loggedIn: false })).toBe("signIn");
    // It left machines behind: the strip says it failed, the header cleans up.
    const left = derive({ startFailed: true, status: status({ machines: [machine("web", "exited")] }) });
    expect(left.phase).toBe("failed");
    expect(headerMode(left, opts)).toBe("interrupted");
    // Started again (or up after all): no longer failed.
    expect(derive({ startFailed: true, busy: true, status: status({}) }).phase).toBe("operation");
    expect(derive({ startFailed: true, status: status({ running: true }) }).phase).toBe("running");
  });
  it("offers Resume only with its handler, and sign-in only when logged out with one", () => {
    expect(headerMode(derive({ status: status({ parked: "pause" }) }), { ...opts, canResume: false })).toBe("start");
    expect(headerMode(derive({ status: status({}) }), { ...opts, loggedIn: false })).toBe("signIn");
    expect(headerMode(derive({ status: status({}) }), { ...opts, loggedIn: false, canLogin: false })).toBe("start");
  });
});

describe("networkView", () => {
  const view = (s: Parameters<typeof derive>[0], o: Partial<Parameters<typeof networkView>[1]> = {}) =>
    networkView(derive(s), { busy: false, acting: null, hasMachines: true, ...o });
  it("draws the diagram once the lab is up or parked", () => {
    expect(view({ status: status({ running: true }) })).toBe("diagram");
    expect(view({ status: status({ parked: "pause" }) })).toBe("diagram");
    expect(view({ status: status({ running: true }) }, { hasMachines: false })).toBe("startToSee");
  });
  it("keeps the diagram through the page's own actions and a stop, hides it during other runs", () => {
    expect(view({ busy: true, acting: "pause", status: status({ running: true }) }, { busy: true, acting: "pause" })).toBe("diagram");
    // A stop (a busy run on a running lab): kept, dimmed, so the page doesn't jump.
    expect(view({ busy: true, status: status({ running: true }) }, { busy: true })).toBe("diagram");
    // A start, whose machines report running before it is done: not yet.
    expect(view({ busy: true, backendOp: "launch", status: status({ running: true }) }, { busy: true })).toBe("none");
    expect(view({ busy: true, status: status({}) }, { busy: true, hasMachines: false })).toBe("none");
  });
  it("says why there is no diagram", () => {
    const leftovers = { machines: [machine("dc01", "poweroff")] };
    const provisionable = { ...leftovers, controls: ["pause", "shutdown", "provision", "repair"] satisfies LabControl[] };
    expect(view({ runtime: vm, status: status(provisionable) }, { hasMachines: false })).toBe("interruptedProvision");
    expect(view({ status: status(leftovers) }, { hasMachines: false })).toBe("interruptedClean");
    expect(view({ status: status({ parked: "shutdown" }) }, { hasMachines: false })).toBe("shutDownNote");
    expect(view({ status: status({ parked: "pause" }) }, { hasMachines: false })).toBe("pausedNote");
    expect(view({ status: status({ running: true, host: "esxi" }) }, { hasMachines: false })).toBe("readingRemote");
  });
});

describe("deployPlacement", () => {
  it("puts the deployment first while it runs, after the diagram once the lab is up", () => {
    expect(deployPlacement(derive({ busy: true, status: status({}) }), { busy: true, failed: false })).toBe("top");
    expect(deployPlacement(derive({ busy: true, backendOp: "launch", status: status({ running: true }) }), { busy: true, failed: false })).toBe("top");
    expect(deployPlacement(derive({ status: status({ running: true }) }), { busy: false, failed: false })).toBe("bottom");
  });
  it("leaves it in place through a stop on a running lab, and after the stop failed", () => {
    const stopping = derive({ busy: true, status: status({ running: true }) });
    expect(stopping.operation).toBe("stop");
    expect(removing(stopping)).toBe(true);
    expect(deployPlacement(stopping, { busy: true, failed: false })).toBe("bottom");
    expect(deployPlacement(derive({ status: status({ running: true }) }), { busy: false, failed: true })).toBe("bottom");
    expect(removing(derive({ busy: true, acting: "pause", status: status({ running: true }) }))).toBe(false);
  });
  it("shows a stopped lab's deployment only when it failed", () => {
    expect(deployPlacement(derive({ status: status({}) }), { busy: false, failed: false })).toBeNull();
    expect(deployPlacement(derive({ status: status({}) }), { busy: false, failed: true })).toBe("top");
  });
});

describe("diagramAbove", () => {
  it("lifts the drawn diagram above the columns on a wide window only", () => {
    expect(diagramAbove(true, "diagram", "bottom")).toBe(true);
    expect(diagramAbove(true, "diagram", null)).toBe(true);
    expect(diagramAbove(false, "diagram", "bottom")).toBe(false);
  });
  it("keeps a note, and the diagram while the deployment leads, in the main column", () => {
    expect(diagramAbove(true, "startToSee", null)).toBe(false);
    expect(diagramAbove(true, "none", "top")).toBe(false);
    expect(diagramAbove(true, "diagram", "top")).toBe(false);
  });
});

describe("restartTarget", () => {
  // What the backend sends: a server or cloud account by its name and saved id; a VM here as
  // place local_vm with no host, the hypervisor as its provider, perMachine for one VM per machine.
  const hosts = [
    { id: "h1", name: "esxi-01" },
    { id: "aws1", name: "My AWS" },
  ] as ServerHost[];
  const here: RunTarget = { kind: "local" };
  it("starts again on the saved host it ran on, found by id", () => {
    expect(restartTarget(status({ host: "esxi-01", hostId: "h1", place: "server" }), hosts, "VM", here, null)).toEqual({ kind: "host", id: "h1" });
  });
  it("starts again on a cloud account in the form it ran in there", () => {
    const vms = status({ host: "My AWS", hostId: "aws1", place: "cloud", perMachine: true });
    expect(restartTarget(vms, hosts, "DOCKER", here, null)).toEqual({ kind: "host", id: "aws1", perMachine: true });
    expect(restartTarget({ ...vms, perMachine: false }, hosts, "DOCKER", here, null)).toEqual({ kind: "host", id: "aws1" });
  });
  it("keeps the pick when the host it ran on is no longer saved (a renamed one is still found)", () => {
    const pick: RunTarget = { kind: "local-vm", provider: "qemu" };
    expect(restartTarget(status({ host: "gone", hostId: "h9", place: "server" }), hosts, "VM", pick, null)).toEqual(pick);
    expect(restartTarget(status({ host: "renamed", hostId: "h1", place: "server" }), hosts, "VM", pick, null)).toEqual({ kind: "host", id: "h1" });
  });
  it("starts a container lab again in one VM here, on the hypervisor it ran on", () => {
    const inVm = status({ place: "local_vm", provider: "qemu" });
    expect(restartTarget(inVm, hosts, "DOCKER", here, "virtualbox")).toEqual({ kind: "local-vm", provider: "qemu" });
  });
  it("starts a container lab run as one VM per machine again in that form", () => {
    const perMachine = status({ place: "local_vm", provider: "virtualbox", perMachine: true });
    expect(restartTarget(perMachine, hosts, "DOCKER", here, "qemu")).toEqual({ kind: "local-vms", provider: "virtualbox" });
  });
  it("falls back to the pick's hypervisor, then the default, when the status names none", () => {
    const inVm = status({ place: "local_vm", provider: null });
    expect(restartTarget(inVm, hosts, "DOCKER", { kind: "local-vm", provider: "parallels" }, "virtualbox")).toEqual({
      kind: "local-vm",
      provider: "parallels",
    });
    expect(restartTarget(inVm, hosts, "DOCKER", here, "virtualbox")).toEqual({ kind: "local-vm", provider: "virtualbox" });
    expect(restartTarget(inVm, hosts, "DOCKER", here, null)).toEqual({ kind: "local" });
  });
  it("starts a VM lab here as its own VMs, and a container lab on its engine", () => {
    expect(restartTarget(status({ place: "local_vm", provider: "virtualbox" }), hosts, "VM", here, null)).toEqual({ kind: "local", provider: "virtualbox" });
    // A status naming no hypervisor of this machine: the VM lab's own VMs, on the backend's pick.
    expect(restartTarget(status({ place: "local_vm", provider: null }), hosts, "VM", here, "qemu")).toEqual({ kind: "local" });
    expect(restartTarget(status({ place: "local_vm", provider: "vmware_esxi" }), hosts, "VM", here, "qemu")).toEqual({ kind: "local" });
    expect(restartTarget(status({ place: "container", provider: "docker" }), hosts, "DOCKER", { kind: "host", id: "h1" }, "virtualbox")).toEqual({
      kind: "local",
    });
  });
});

describe("publishedBinds", () => {
  it("lists each published port with its machine", () => {
    const machines = [
      machine("web", "running", {
        ports: [
          { published: 5000, target: 80 },
          { published: 0, target: 3306 },
        ],
      }),
    ];
    expect(publishedBinds(machines)).toEqual([{ machine: "web", target: 80, published: 5000 }]);
  });
});

describe("bindHost", () => {
  it("is the URL's host, else loopback", () => {
    expect(bindHost("http://10.1.2.3:8080/x")).toBe("10.1.2.3");
    expect(bindHost(null)).toBe("127.0.0.1");
    expect(bindHost("not a url")).toBe("127.0.0.1");
  });
});

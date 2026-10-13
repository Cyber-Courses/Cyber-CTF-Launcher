import { describe, expect, it } from "vitest";

import {
  bindHost,
  deployPlacement,
  diagramAbove,
  deriveLabState,
  headerMode,
  networkView,
  publishedBinds,
  restartTarget,
  type LabStateInput,
} from "@/features/labs/lab-detail/lab-state";
import type { LabRuntimeInfo } from "@/features/labs/use-labs";
import type { LabMachine, LabStatus, Provider, ServerHost } from "@/lib/tauri";

const docker: LabRuntimeInfo = { runtime: "DOCKER", architectures: [], providers: [] };
const vm: LabRuntimeInfo = { runtime: "VM", architectures: ["x86_64"], providers: ["virtualbox"] as Provider[] };

const machine = (name: string, state = "running", p: Partial<LabMachine> = {}): LabMachine => ({
  name,
  state,
  image: "",
  ip: "",
  ports: [],
  interfaces: [],
  services: [],
  infra: false,
  ...p,
});
const status = (p: Partial<LabStatus>): LabStatus => ({
  running: false,
  parked: null,
  machines: [],
  networks: [],
  url: null,
  host: null,
  expiresAt: null,
  place: null,
  provider: null,
  attacker: null,
  outputs: [],
  message: null,
  ...p,
});
const derive = (p: Partial<LabStateInput>) =>
  deriveLabState({
    status: undefined,
    runtime: docker,
    hostArch: "x86_64",
    readyVms: [],
    busy: false,
    backendDeploying: false,
    backendOp: undefined,
    acting: null,
    parkable: true,
    provisionable: true,
    ...p,
  });

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

  it("offers pause and shut down by where the lab runs", () => {
    const here = derive({ status: status({ running: true, place: "container" }) });
    expect([here.canPause, here.canShutdown]).toEqual([false, true]);
    const inVm = derive({ status: status({ running: true, place: "local_vm" }) });
    expect([inVm.canPause, inVm.canShutdown]).toEqual([true, false]);
    const vmLab = derive({ runtime: vm, status: status({ running: true, place: "local_vm" }) });
    expect([vmLab.canPause, vmLab.canShutdown]).toEqual([true, false]);
    const remote = derive({ runtime: vm, status: status({ running: true, host: "esxi", place: "server" }) });
    expect([remote.canPause, remote.canShutdown]).toEqual([false, false]);
    const noHandler = derive({ parkable: false, status: status({ running: true, place: "container" }) });
    expect([noHandler.canPause, noHandler.canShutdown]).toEqual([false, false]);
  });

  it("re-runs provisioning on Vagrant VM labs only", () => {
    expect(derive({ runtime: vm, status: status({ place: "local_vm" }) }).canProvision).toBe(true);
    expect(derive({ runtime: vm, status: status({ place: "server", provider: "vmware_esxi" }) }).canProvision).toBe(true);
    expect(derive({ runtime: vm, status: status({ place: "server", provider: "proxmox" }) }).canProvision).toBe(false);
    expect(derive({ runtime: vm, status: status({ place: "cloud" }) }).canProvision).toBe(false);
    expect(derive({ runtime: docker, status: status({}) }).canProvision).toBe(false);
    expect(derive({ runtime: vm, provisionable: false, status: status({}) }).canProvision).toBe(false);
  });

  it("names the engine or hypervisor it runs on", () => {
    expect(derive({ status: status({ provider: "docker" }) }).engine).toBe("Docker");
    expect(derive({ status: status({ provider: "virtualbox" }) }).engine).toBe("VirtualBox");
    expect(derive({ status: status({ provider: "something" }) }).engine).toBe("something");
    expect(derive({ status: status({}) }).engine).toBeNull();
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
  it("keeps the diagram through the page's own actions, hides it during other runs", () => {
    expect(view({ busy: true, acting: "pause", status: status({ running: true }) }, { busy: true, acting: "pause" })).toBe("diagram");
    expect(view({ busy: true, status: status({ running: true }) }, { busy: true })).toBe("none");
    expect(view({ busy: true, status: status({}) }, { busy: true, hasMachines: false })).toBe("none");
  });
  it("says why there is no diagram", () => {
    const leftovers = { machines: [machine("dc01", "poweroff")] };
    expect(view({ runtime: vm, status: status(leftovers) }, { hasMachines: false })).toBe("interruptedProvision");
    expect(view({ status: status(leftovers) }, { hasMachines: false })).toBe("interruptedClean");
    expect(view({ status: status({ parked: "shutdown" }) }, { hasMachines: false })).toBe("shutDownNote");
    expect(view({ status: status({ parked: "pause" }) }, { hasMachines: false })).toBe("pausedNote");
    expect(view({ status: status({ running: true, host: "esxi" }) }, { hasMachines: false })).toBe("readingRemote");
  });
});

describe("deployPlacement", () => {
  it("puts the deployment first while it runs, after the diagram once the lab is up", () => {
    expect(deployPlacement(derive({ busy: true, status: status({}) }), { busy: true, failed: false })).toBe("top");
    expect(deployPlacement(derive({ busy: true, status: status({ running: true }) }), { busy: true, failed: false })).toBe("top");
    expect(deployPlacement(derive({ status: status({ running: true }) }), { busy: false, failed: false })).toBe("bottom");
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
  const hosts = [{ id: "h1", name: "esxi-01" }] as ServerHost[];
  it("starts again on the saved host it ran on", () => {
    expect(restartTarget(status({ host: "esxi-01" }), hosts, { kind: "local" }, null)).toEqual({ kind: "host", id: "h1" });
  });
  it("starts again in a VM here when its host isn't a saved one", () => {
    expect(restartTarget(status({ host: "isoloom-host" }), hosts, { kind: "local-vm", provider: "qemu" as Provider }, "virtualbox" as Provider)).toEqual({
      kind: "local-vm",
      provider: "qemu",
    });
    expect(restartTarget(status({ host: "isoloom-host" }), hosts, { kind: "local" }, "virtualbox" as Provider)).toEqual({
      kind: "local-vm",
      provider: "virtualbox",
    });
    expect(restartTarget(status({ host: "isoloom-host" }), hosts, { kind: "local" }, null)).toEqual({ kind: "local" });
  });
  it("starts again here otherwise", () => {
    expect(restartTarget(status({}), hosts, { kind: "host", id: "h1" }, "virtualbox" as Provider)).toEqual({ kind: "local" });
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

import { describe, expect, it } from "vitest";

import { bindHost, cloudView, deployPlacement, headerMode, networkView, publishedBinds, restartTarget } from "@/features/labs/lab-detail/lab-state";
import type { LabRuntimeInfo } from "@/features/labs/use-labs";
import type { Provider } from "@/lib/tauri";
import { deriveState as derive, labMachine as machine, labStatus as status } from "@/features/labs/labs-test-kit";

const vm: LabRuntimeInfo = { runtime: "VM", architectures: ["x86_64"], providers: ["virtualbox", "vmware_esxi", "proxmox"] as Provider[] };
const cloud: LabRuntimeInfo = { runtime: "CLOUD", architectures: [], providers: ["aws"] as Provider[] };

// Inputs that contradict each other (a status that is both running and parked, a failed deploy
// on a running lab) and a cloud lab, which has no machines to read from.
describe("deriveLabState: contradictory inputs", () => {
  it("a status both running and parked reads as running, with no park", () => {
    const s = derive({ status: status({ running: true, parked: "pause", machines: [machine("web")] }) });
    expect(s.parked).toBeNull();
    expect(s.phase).toBe("running");
    expect(headerMode(s, { loggedIn: true, canLogin: true, canResume: true })).toBe("running");
  });

  it("a running lab whose last deploy failed keeps its deployment panel, under the diagram", () => {
    const running = derive({ status: status({ running: true }) });
    expect(deployPlacement(running, { busy: false, failed: true })).toBe("bottom");
    const stopped = derive({ status: status({}) });
    expect(deployPlacement(stopped, { busy: false, failed: true })).toBe("top");
    expect(deployPlacement(stopped, { busy: false, failed: false })).toBeNull();
  });

  it("a cloud lab is interrupted by an account left recorded, never by machines, and offers no pause, shut down or provisioning", () => {
    const idle = derive({ runtime: cloud, status: status({ machines: [machine("x", "exited")] }) });
    expect(idle.interrupted).toBe(false);
    expect(idle.phase).toBe("notStarted");
    expect(cloudView(idle, { busy: false })).toBe("cloudIdle");
    const left = derive({ runtime: cloud, status: status({ provider: "aws" }) });
    expect(left.interrupted).toBe(true);
    expect(cloudView(left, { busy: false })).toBe("cloudLeftover");
    expect([left.canShutdown, left.canProvision]).toEqual([false, false]);
    expect(cloudView(derive({ runtime: cloud, busy: true }), { busy: true })).toBe("none");
  });

  it("a running cloud lab offers no Pause, which the backend would refuse", () => {
    // `isDocker` is false for a cloud lab, so canPause takes the VM branch and is true whenever
    // the page wires onPark (labs-screen.tsx always does): the header shows Pause on a running
    // cloud-services lab, and the click fails with lifecycle.rs's "A cloud lab can't be paused".
    // canShutdown already excludes cloud labs. Suggested fix: `&& !isCloud` in canPause.
    const s = derive({ runtime: cloud, status: status({ running: true, provider: "aws" }) });
    expect(s.canPause).toBe(false);
  });

  it("a parked lab that is busy is being stopped, unless the page says it resumes", () => {
    const stopping = derive({ busy: true, status: status({ parked: "pause" }) });
    expect(stopping.operation).toBe("stop");
    expect(stopping.tearingDown).toBe(true);
    expect(stopping.parked).toBeNull();
    const resuming = derive({ busy: true, acting: "resume", status: status({ parked: "pause" }) });
    expect(resuming.operation).toBe("resume");
    expect(resuming.phase).toBe("operation");
  });

  it("an attack VM start reported by the backend on a running lab keeps the running word and tears nothing down", () => {
    const s = derive({ backendDeploying: true, backendOp: "attack_vm", status: status({ running: true, machines: [machine("web")] }) });
    expect(s.operation).toBe("attack_vm");
    expect(s.phase).toBe("running");
    expect(s.tearingDown).toBe(false);
    expect(s.down).toEqual([]);
  });

  it("with no host CPU known yet, a lab built for another CPU is native, and QEMU ready still means it emulates", () => {
    const foreign: LabRuntimeInfo = { ...vm, architectures: ["aarch64"] };
    const s = derive({ runtime: foreign, hostArch: "", readyVms: ["qemu"] });
    expect(s.native).toBe(true);
    expect(s.emulates).toBe(true);
    expect(derive({ runtime: foreign, hostArch: "x86_64", readyVms: ["virtualbox"] }).native).toBe(false);
  });

  it("a lab on a server offers neither pause nor shut down, and provisioning only off Proxmox", () => {
    const esxi = derive({ runtime: vm, status: status({ running: true, host: "esxi-1", provider: "vmware_esxi" }) });
    expect([esxi.remote, esxi.canPause, esxi.canShutdown, esxi.canProvision]).toEqual([true, false, false, true]);
    expect(esxi.engine).toBe("VMware ESXi (remote)");
    const pve = derive({ runtime: vm, status: status({ running: true, host: "pve", provider: "proxmox" }) });
    expect(pve.canProvision).toBe(false);
    expect(derive({ runtime: vm, status: status({ provider: "weird" }) }).engine).toBe("weird");
  });

  it("a parked lab lists none of its machines as down", () => {
    // `down` is meant for machines that went down while the lab runs (its comment says so),
    // but it only skips the deploying case: a paused lab's machines are all "paused", a shut-down
    // lab's all "poweroff", and both land in `down`. Harmless while the page gates the health
    // panel on `running`; wrong as a derived fact. Suggested fix: `down = deploying || !running ? [] : …`.
    const paused = derive({ status: status({ parked: "pause", machines: [machine("web", "paused"), machine("db", "paused")] }) });
    expect(paused.phase).toBe("paused");
    expect(paused.down).toEqual([]);
  });
});

describe("headerMode and networkView: edges", () => {
  it("a parked lab without a Resume handler offers Start, or sign-in; an interrupted run comes before sign-in", () => {
    const parked = derive({ status: status({ parked: "shutdown" }) });
    expect(headerMode(parked, { loggedIn: false, canLogin: true, canResume: false })).toBe("signIn");
    expect(headerMode(parked, { loggedIn: true, canLogin: true, canResume: false })).toBe("start");
    const interrupted = derive({ status: status({ machines: [machine("web", "exited")] }) });
    expect(headerMode(interrupted, { loggedIn: false, canLogin: true, canResume: true })).toBe("interrupted");
  });

  it("a parked lab with no machines reported shows the parked note, a remote running lab without them is being read", () => {
    const shut = derive({ status: status({ parked: "shutdown" }) });
    expect(networkView(shut, { busy: false, acting: null, hasMachines: false })).toBe("shutDownNote");
    expect(networkView(shut, { busy: false, acting: null, hasMachines: true })).toBe("diagram");
    const remote = derive({ runtime: vm, status: status({ running: true, host: "esxi-1" }) });
    expect(networkView(remote, { busy: false, acting: null, hasMachines: false })).toBe("readingRemote");
  });

  it("an interrupted VM lab offers provisioning, an interrupted container lab a clean-up", () => {
    const vmLab = derive({ runtime: vm, status: status({ machines: [machine("dc01", "poweroff")] }) });
    expect(networkView(vmLab, { busy: false, acting: null, hasMachines: true })).toBe("interruptedProvision");
    const dockerLab = derive({ status: status({ machines: [machine("web", "exited")] }) });
    expect(networkView(dockerLab, { busy: false, acting: null, hasMachines: true })).toBe("interruptedClean");
  });
});

describe("bindHost, publishedBinds, restartTarget: edges", () => {
  it("reads an IPv6 literal and a host behind credentials, and falls back to loopback for anything unparsable", () => {
    expect(bindHost("http://[::1]:8080/")).toBe("[::1]");
    expect(bindHost("http://user:pw@10.0.0.5:81/x")).toBe("10.0.0.5");
    expect(bindHost("")).toBe("127.0.0.1");
    expect(bindHost("::nope")).toBe("127.0.0.1");
  });

  it("a port Docker reports once per address family is listed once", () => {
    // Docker publishes on 0.0.0.0 and :: and the status carries both entries; the diagram
    // dedupes them (uniquePorts) but the Details panel does not, so it draws the same row
    // twice with the same React key (details-panel keys rows by machine:target). Suggested fix:
    // dedupe on (machine, target, published) in publishedBinds.
    const web = machine("web", "running", {
      ports: [
        { published: 8080, target: 80 },
        { published: 8080, target: 80 },
      ],
    });
    expect(publishedBinds([web])).toEqual([{ machine: "web", target: 80, published: 8080 }]);
  });

  it("falls back to this machine when the status names a host that isn't saved and no local hypervisor is known", () => {
    expect(restartTarget(status({ host: "ESXI-1" }), [{ id: "h", name: "esxi-1" } as never], { kind: "host", id: "h" }, null)).toEqual({ kind: "local" });
    expect(restartTarget(status({ host: "ESXI-1" }), [], { kind: "local" }, "qemu")).toEqual({ kind: "local-vm", provider: "qemu" });
  });
});

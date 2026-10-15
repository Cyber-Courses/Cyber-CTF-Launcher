// What the browser preview changes as it is used: labs started and stopped, where they run, the
// sign-in, the failures still to come, the steps a test still holds. And the statuses the labs
// report meanwhile.
import type { LabStatus } from "@/lib/tauri";
import { argText, send, sleep } from "@/lib/dev-mock/command";
import { type Holds, holdsOf, isHoldable } from "@/lib/dev-mock/holds";
import type { Host } from "@/lib/dev-mock/hosts";
import { invoiceStatus, labs, stopped } from "@/lib/dev-mock/labs";
import { answer } from "@/lib/dev-mock/sample";
import type { Scenario } from "@/lib/dev-mock/scenario";
import type { Args } from "@/lib/dev-mock/types";

/** Where a VM or cloud lab started in the preview runs, and whether it is parked. */
type Placement = { provider: string; host: Host | null; parked: "pause" | "shutdown" | null; startedAt: number };

/** A hosted session (Cyber CTF runs the lab), as CyberBackend reports it. */
type Session = {
  id: string;
  labId: string;
  target: "hosted";
  state: string;
  endpoints: { port: number; url: string }[];
  message: null;
  expiresAt: string;
};

/** An operation the core reports in flight on a lab. */
type Operation = "launch" | "stop" | "pause" | "shutdown" | "resume" | "provision";

export type Live = {
  running: Set<string>;
  signedIn: boolean;
  launchFails: boolean;
  /** The operations in flight on each lab, as the core reports them. */
  ops: Map<string, Operation>;
  /** Tools the one-click installs added (dependencies, Vagrant plugins). */
  installed: Set<string>;
  installFails: boolean;
  /** The steps the test holds (`hold=`) until it lets them go on. */
  holds: Holds;
  /** VM and cloud labs started here: where they run. */
  placed: Map<string, Placement>;
  /** The hosted session, once one is requested. */
  hosted: Session | null;
};

/** The preview's state at load, as the scenario sets it. */
export function liveFrom(sc: Scenario): Live {
  return {
    running: new Set(answer("running_labs", undefined) as string[]),
    signedIn: !sc.signedOut,
    launchFails: sc.launchFails,
    ops: new Map(),
    installed: new Set(),
    installFails: sc.installFails,
    holds: holdsOf(sc.held),
    placed: new Map(),
    hosted: null,
  };
}

/** What a browser handler sees: the scenario, and the state it changes. */
export type Context = { sc: Scenario; live: Live };

/** A lab of the catalogue, by id. */
const labById = (id: string) => labs.find((l) => l.id === id);
/** A lab's slug (its id for one the catalogue doesn't have). */
export const slugOf = (id: string) => labById(id)?.slug ?? id;
/** How a lab runs: DOCKER, VM or CLOUD. */
export const labRuntime = (id: string) => labById(id)?.runtime.runtime;
/** The lab an operation names (`id`, or `labId` for a launch). */
export const labArg = (args: Args, key: "id" | "labId" = "id") => argText(args?.[key]);

/** An operation `op` on the lab `id`: in flight (as the core reports it) while its `lines`
 *  stream down the command's channel, and then while the test holds it (`hold=launch`,
 *  `hold=provision`). */
export async function operation({ id, op, lines }: { id: string; op: Operation; lines: string[] }, args: Args, live: Live) {
  live.ops.set(id, op);
  try {
    for (const line of lines) {
      send(args, line);
      await sleep(120);
    }
    if (isHoldable(op)) await live.holds.passed(op);
  } finally {
    live.ops.delete(id);
  }
}

/** A VM lab's machines (GOAD Light's, for any VM lab). */
const VM_MACHINES = [
  { name: "dc01", ip: "192.168.56.10", image: "StefanScherer/windows_2019" },
  { name: "srv02", ip: "192.168.56.22", image: "StefanScherer/windows_2019" },
];

/** The VM lab machines an operation acts on: `machine`, or all of them. */
export const vagrantMachines = (machine?: string | null) => VM_MACHINES.filter((m) => !machine || m.name === machine).map((m) => m.name);

const hostName = (p: Placement) => (p.host ? argText(p.host.name) : null);

/** A VM lab's status: its machines on the lab network, running, saved or powered off. */
function vmStatus(p: Placement): LabStatus {
  const state = p.parked === "pause" ? "saved" : p.parked === "shutdown" ? "poweroff" : "running";
  return {
    ...stopped,
    running: !p.parked,
    parked: p.parked,
    host: hostName(p),
    place: p.host ? "server" : "local_vm",
    provider: p.provider,
    networks: [{ name: "lab", subnet: "192.168.56.0/24", internal: true }],
    machines: VM_MACHINES.map((m) => ({
      name: m.name,
      state,
      image: m.image,
      ip: m.ip,
      ports: [],
      interfaces: [{ network: "lab", ip: m.ip }],
      services: [{ name: "ldap", kind: "directory", ports: [389] }],
      infra: false,
    })),
  };
}

/** A cloud lab's status: up in the account, with its outputs and when it stops itself. */
function cloudStatus(p: Placement): LabStatus {
  const hours = Number(p.host?.autoStopHours ?? 0);
  return {
    ...stopped,
    running: true,
    host: hostName(p),
    place: "cloud",
    provider: p.provider,
    expiresAt: hours ? Math.floor(p.startedAt / 1000) + hours * 3600 : null,
    outputs: [
      { name: "access_key_id", value: "AKIAZ7EXAMPLE4SSRF01", sensitive: false },
      { name: "secret_key", value: "wJalrXUtnFEMI/K7MDENG/bPxRfiCYSSRF", sensitive: true },
    ],
    message: "Sign in with access_key_id and secret_key, then find the Lambda function.",
  };
}

/** A started container lab's status: one web container on its own network, next to the attack box. */
function startedStatus(id: string): LabStatus {
  if (id === "lab-invoice") return invoiceStatus;
  return {
    ...invoiceStatus,
    url: "http://127.0.0.1:8443",
    networks: [{ name: "default", subnet: "10.43.0.0/24", internal: false }],
    machines: [
      {
        name: "web",
        state: "running",
        image: `${slugOf(id)}:latest`,
        ip: "10.43.0.2",
        ports: [{ published: 8443, target: 443 }],
        interfaces: [{ network: "default", ip: "10.43.0.2" }],
        services: [{ name: "dashboard", kind: "web", ports: [443] }],
        infra: false,
      },
    ],
  };
}

/** A lab's status as the preview has it: where it was placed, started here, or stopped. */
export function statusOf(id: string, live: Live): LabStatus {
  const placed = live.placed.get(id);
  if (placed) return labRuntime(id) === "CLOUD" ? cloudStatus(placed) : vmStatus(placed);
  return live.running.has(id) ? startedStatus(id) : stopped;
}

/** A command's answer in the browser preview, given its arguments and the preview's state. */
export type BrowserHandler = (args: Args, ctx: Context) => unknown;

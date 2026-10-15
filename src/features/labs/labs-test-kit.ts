// Fixtures the labs tests share: a lab, its status and machines as the core reports them, the lab
// page's derived state, and a command failing as the core fails. Not part of the app: only tests
// import it.

import { screen, within } from "@testing-library/react";
import type { UserEvent } from "@testing-library/user-event";
import { deriveLabState, type LabStateInput } from "@/features/labs/lab-detail/lab-state";
import type { Lab } from "@/features/labs/use-labs";
import { stopped } from "@/lib/dev-mock/labs";
import type { LabMachine, LabStatus } from "@/lib/tauri";

// Fails a mocked Tauri command as the Rust core does (shared by every test).
export { rejectWith } from "@/test/tauri";

/** A catalogue lab `id` (its slug and title too): a level 1 Web lab in Docker, `p` over it. */
export const testLab = (id: string, p: Partial<Lab> = {}): Lab => ({
  id,
  slug: id,
  title: id,
  description: null,
  question: null,
  difficulty: 1,
  category: "Web",
  runtime: { runtime: "DOCKER", architectures: [], providers: [] },
  skills: [],
  ...p,
});

/** The invoice portal lab as a list row shows it: level 2, Docker on x86_64. */
export const invoiceRowLab = testLab("lab-invoice", {
  slug: "invoice-portal-api",
  title: "Invoice portal API",
  description: "A billing portal",
  difficulty: 2,
  runtime: { runtime: "DOCKER", architectures: ["x86_64"], providers: [] },
});

/** A lab's status as the core reports a lab that isn't running, `p` over it. */
export const labStatus = (p: Partial<LabStatus> = {}): LabStatus => ({ ...structuredClone(stopped), ...p });

/** A machine the core reports by its name and state only, `p` over it. */
export const labMachine = (name: string, state = "running", p: Partial<LabMachine> = {}): LabMachine => ({
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

/** A `node:20` container on the lab's default network, at 10.42.0.3, `p` over it. */
export const containerMachine = (name: string, state = "running", p: Partial<LabMachine> = {}): LabMachine =>
  labMachine(name, state, { image: "node:20", ip: "10.42.0.3", interfaces: [{ network: "default", ip: "10.42.0.3" }], ...p });

/** The lab page's state for a settled Docker lab on an x86_64 machine, `p` over its inputs. */
export const deriveState = (p: Partial<LabStateInput>) =>
  deriveLabState({
    status: undefined,
    runtime: { runtime: "DOCKER", architectures: [], providers: [] },
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

/** A Docker lab up on this machine: `machines` on its default network (10.42.0.0/24), served on
 *  port 8080, `p` over it. */
export const upStatus = (machines: LabMachine[], p: Partial<LabStatus> = {}): LabStatus =>
  labStatus({
    running: true,
    machines,
    networks: [{ name: "default", subnet: "10.42.0.0/24", internal: false }],
    url: "http://127.0.0.1:8080",
    place: "container",
    provider: "docker",
    ...p,
  });

/** The titles of the lab rows on screen, in order (read from their Open buttons). */
export const rowTitles = () =>
  screen
    .queryAllByRole("button")
    .map((b) => b.getAttribute("aria-label") ?? "")
    .filter((l) => l.startsWith("Open "))
    .map((l) => l.slice(5));

/** In the open "Where should it run?" dialog: picks `place`, then starts the lab there. */
export async function startIn(user: UserEvent, place: string) {
  await user.click(within(screen.getByRole("dialog")).getByText(place));
  await user.click(within(screen.getByRole("dialog")).getByRole("button", { name: /^Start$/ }));
}

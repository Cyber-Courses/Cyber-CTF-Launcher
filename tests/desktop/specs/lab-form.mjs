// A catalogue lab started on this machine in one of its forms, through its page's real "Where
// should it run?" dialog, until its page says it runs; what runs checked on the engine itself
// (its containers in Docker, its VMs in the hypervisor's own list), its published address
// answering, then Stop & remove from the page (the system confirmation answered) and nothing of it
// left. One form a run (`CYBERCTF_TEST_LAB_FORM`), so the runner gives each its own verdict:
//
//   container    the lab as containers on this machine's engine
//   docker-vm    its containers in one VM (the helper VM), on CYBERCTF_TEST_VM_PROVIDER
//   per-machine  one VM per machine, from its VM recipes, on CYBERCTF_TEST_VM_PROVIDER
//   vm           a VM lab's own VMs (CYBERCTF_TEST_CATALOGUE_VM_LAB), on CYBERCTF_TEST_VM_PROVIDER
//
// Needs the app signed in (a catalogue lab starts only so; a debug build reads its session from
// ~/.cyberctf/dev-session.json, which the runner can place from CYBERCTF_TEST_ACCOUNT_SESSION). The
// labs' own checks, status and park/resume per form are the integration tests' (docker-lab,
// docker-vm-lab, vm-lab, per-machine); an imported lab's start is labs' and launch-ui's.
import {
  CATALOGUE_LAB,
  docker,
  dockerUp,
  finish,
  hypervisorVms,
  mainButton,
  mainText,
  openApp,
  openLabFromList,
  results,
  signedIn,
  sleep,
  startFromDialog,
  stopAndRemoveOnPage,
  stopLeftover,
  waitLabPageRunning,
} from "../lib/support.mjs";

const r = results();
const FORM = process.env.CYBERCTF_TEST_LAB_FORM || "container";
const PROVIDER = process.env.CYBERCTF_TEST_VM_PROVIDER || (process.platform === "darwin" ? "utm" : "");
const FORMS = {
  container: { place: "container", lab: CATALOGUE_LAB, minutes: 20 },
  "docker-vm": { place: "vm", form: "one", lab: CATALOGUE_LAB, minutes: 45 },
  "per-machine": { place: "vm", form: "per-machine", lab: CATALOGUE_LAB, minutes: 60 },
  vm: { place: "vm", form: "one", lab: process.env.CYBERCTF_TEST_CATALOGUE_VM_LAB || "", minutes: 60 },
};
const want = FORMS[FORM];
if (!want) r.skipAll(`unknown CYBERCTF_TEST_LAB_FORM "${FORM}" (container, docker-vm, per-machine or vm)`);
if (!want.lab) r.skipAll("no VM lab named: set CYBERCTF_TEST_CATALOGUE_VM_LAB to a catalogue VM lab's title");
if (want.place === "container" && !dockerUp()) r.skipAll("no Docker engine answers here");
if (want.place === "vm" && !PROVIDER) r.skipAll("no hypervisor named: set CYBERCTF_TEST_VM_PROVIDER (libvirt, virtualbox, hyperv, qemu, utm…)");

/** What runs of labs here: the lab containers (`cyberctf-*`) running, and the hypervisor's VMs. */
function snapshot() {
  const containers = (docker("ps", "--format", "{{.Names}}", "--filter", "name=^cyberctf-") ?? "").split("\n").filter(Boolean);
  return { containers, vms: want.place === "vm" ? hypervisorVms(PROVIDER) : null };
}
const added = (now, before) => now.filter((n) => !before.includes(n));

let proc;
let b;
try {
  ({ proc, b } = await openApp());
  if (!(await signedIn(b))) r.skipAll("the app isn't signed in: a catalogue lab starts only signed in (give CYBERCTF_TEST_ACCOUNT_SESSION to the runner)");
  if (!(await openLabFromList(b, want.lab))) r.skipAll(`the catalogue doesn't list "${want.lab}"`);
  await stopLeftover(b, r);
  const before = snapshot();

  const started = await r.step(
    "start",
    `Start lab, ${FORM} through the dialog`,
    async () => `picked: ${await startFromDialog(b, { ...want, provider: PROVIDER })}`,
  );
  const running = started && (await r.step("running", `the page says ${want.lab} runs`, () => waitLabPageRunning(b, want.minutes * 60000)));
  if (running) {
    await r.step("engine", `what runs is the ${FORM} form, asked of the engine`, async () => {
      const now = snapshot();
      if (want.place === "container") {
        const mine = added(now.containers, before.containers);
        if (mine.length === 0) throw new Error("no new lab container runs in Docker");
        return `containers: ${mine.join(", ")}`;
      }
      if (now.vms === null) return `${PROVIDER} can't be asked for its VMs here: the page's state is the evidence`;
      const mine = added(now.vms, before.vms ?? []);
      if (mine.length === 0) throw new Error(`no new VM in ${PROVIDER}: ${now.vms.join(", ")}`);
      if (FORM === "per-machine" && mine.length < 2) throw new Error(`one VM per machine, but only ${mine.join(", ")}`);
      return `VMs: ${mine.join(", ")}`;
    });
    const open = await mainButton(b, "Open in browser");
    if (await open.isExisting()) {
      const url = ((await open.getAttribute("title")) ?? "").match(/https?:\/\/\S+/)?.[0];
      await r.step("address", `its address answers`, async () => {
        const status = await fetch(url).then(
          (res) => res.status,
          (e) => `error ${e.message}`,
        );
        if (typeof status !== "number" || status >= 500) throw new Error(`${url} -> ${status}`);
        return `${url} -> HTTP ${status}`;
      });
    } else r.skip("address", "the lab publishes no address on this machine");
  } else {
    r.skip("engine", "the lab never ran");
    r.skip("address", "the lab never ran");
  }

  await r.step("stop", "Stop & remove from the page leaves nothing of the lab", async () => {
    if (await (await mainButton(b, "Stop & remove", "Stop & clean up")).isExisting()) await stopAndRemoveOnPage(b, want.minutes * 60000);
    await sleep(3000);
    const after = snapshot();
    const containers = added(after.containers, before.containers);
    const vms = after.vms && before.vms ? added(after.vms, before.vms) : [];
    if (containers.length || vms.length) throw new Error(`left: ${[...containers, ...vms].join(", ")}`);
  });
} catch (e) {
  r.fail("error", e.message);
  if (b) console.log("main text:", JSON.stringify((await mainText(b)).slice(0, 800)));
  if (proc) console.log("app out tail:", proc.output().slice(-600));
} finally {
  await finish(b, proc);
}
r.exit();

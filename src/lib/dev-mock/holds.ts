// Steps of the browser preview that a test holds (`hold=` in the URL): once reached, each waits
// until the test lets it go on (`window.__CYBERCTF_MOCK_RELEASE__(step)`). What the UI shows while
// the step lasts then stays on screen for as long as the test looks at it, however slow the
// machine running the test, instead of racing the step's own timing or a background poll. It is
// the preview's one such mechanism: another step a test needs to time joins HOLDABLE.

/** The steps a test can hold:
 *  - `launch`: a lab start stays in flight before the lab runs (a container lab's containers
 *    created, not started; a VM lab's machines up, not reported running yet);
 *  - `provision`: a re-run of a VM lab's provisioning, likewise;
 *  - `hosted`: a hosted session stays requested, however often it is polled;
 *  - `check`: every system check fails, as on a machine whose engine doesn't answer, until the
 *    machine comes back (once released; a check in flight then answers like the next one);
 *  - `pm`: the package manager stays missing from the system check (as if the player ran its
 *    install command while the setup window was open, once released). */
const HOLDABLE = ["launch", "provision", "hosted", "check", "pm"] as const;

/** A step a test can hold. */
export type Holdable = (typeof HOLDABLE)[number];

/** Whether `step` names a step a test can hold. */
export const isHoldable = (step: string): step is Holdable => (HOLDABLE as readonly string[]).includes(step);

/** The held steps of a page load. */
export type Holds = {
  /** Whether `step` is held now: named in the URL, and not released yet. */
  isHeld: (step: Holdable) => boolean;
  /** Settles once `step` may go on: at once if it isn't held. */
  passed: (step: Holdable) => Promise<void>;
  /** Lets `step` go on, now and from then on. Throws for a step the URL doesn't hold, so a test
   *  releasing one it forgot to hold fails instead of passing on timing. */
  release: (step: string) => void;
};

type Gate = { open: boolean; opened: Promise<void>; openIt: () => void };

function closedGate(): Gate {
  let resolve = () => {};
  const opened = new Promise<void>((r) => {
    resolve = r;
  });
  const gate: Gate = {
    open: false,
    opened,
    openIt: () => {
      gate.open = true;
      resolve();
    },
  };
  return gate;
}

/** The holds for the steps named in the URL, all closed. */
export function holdsOf(steps: readonly Holdable[]): Holds {
  const gates = new Map(steps.map((step) => [step, closedGate()] as const));
  return {
    isHeld: (step) => gates.get(step)?.open === false,
    passed: (step) => gates.get(step)?.opened ?? Promise.resolve(),
    release: (step) => {
      const gate = isHoldable(step) ? gates.get(step) : undefined;
      if (!gate) throw new Error(`the preview doesn't hold '${step}': add hold=${step} to the URL (it holds ${HOLDABLE.join(", ")})`);
      gate.openIt();
    },
  };
}

import type { ELK, ElkNode } from "elkjs/lib/elk-api";
import { isReady } from "@/features/labs/network-diagram/elk-ready";

// ELK (~1.4 MB) is loaded on the first layout, not with the app. In a browser it runs in a Web
// Worker (elk.worker.ts, a same-origin script the CSP allows), off the main thread. Without
// workers (tests), or if the worker doesn't start, it runs on the main thread from the bundled
// build. The choice is made once and logged; falling back is remembered for the session, so a
// webview whose worker doesn't start pays the wait once.

/** How long the worker gets to load and say it is ready. */
export const READY_MS = 2_000;
/** How long one layout may take in a worker that started, before it is laid out on the main
 *  thread instead (and the worker given up for the session). */
export const LAYOUT_MS = 15_000;
/** sessionStorage: set once the worker was given up. */
export const MAIN_THREAD_KEY = "cyberctf.elk.mainThread";

type Engine = { layout: (graph: ElkNode) => Promise<ElkNode> };

let engine: Promise<Engine> | undefined;

const remembered = () => {
  try {
    return sessionStorage.getItem(MAIN_THREAD_KEY) === "1";
  } catch {
    return false;
  }
};
const remember = () => {
  try {
    sessionStorage.setItem(MAIN_THREAD_KEY, "1");
  } catch {
    /* storage unavailable: this page still remembers it */
  }
};

async function inline(): Promise<ELK> {
  const { default: Elk } = await import("elkjs/lib/elk.bundled.js");
  return new Elk();
}

/** ELK in a worker, once the worker said it is ready; rejects if it doesn't within READY_MS
 *  (or fails to load, or can't be created). */
async function inWorker(): Promise<ELK> {
  const { default: ElkApi } = await import("elkjs/lib/elk-api.js");
  let elk: ELK | undefined;
  try {
    return await new Promise<ELK>((resolve, reject) => {
      const slow = setTimeout(() => {
        reject(new Error("the layout worker did not start"));
      }, READY_MS);
      const settle = (f: () => void) => {
        clearTimeout(slow);
        f();
      };
      // The worker's messages come after this constructor returned: `api` is set by then.
      const api: ELK = new ElkApi({
        workerFactory: () => {
          const worker = new Worker(new URL("./elk.worker.ts", import.meta.url));
          worker.addEventListener("message", (e) => {
            if (isReady(e.data))
              settle(() => {
                resolve(api);
              });
          });
          worker.addEventListener("error", (e) => {
            settle(() => {
              reject(new Error(e.message || "the layout worker failed to load"));
            });
          });
          return worker;
        },
      });
      elk = api;
    });
  } catch (e) {
    elk?.terminateWorker();
    throw e;
  }
}

/** The main-thread engine, remembered for the session. */
function mainThread(why: string): Promise<Engine> {
  remember();
  console.warn(`network diagram: ELK runs on the main thread (${why})`);
  return inline();
}

/** A started worker, each layout bounded by LAYOUT_MS: past it, the worker is given up and the
 *  layout (and the next ones) run on the main thread. */
function bounded(elk: ELK): Engine {
  let givenUp: Promise<Engine> | undefined;
  const giveUp = () => {
    if (!givenUp) {
      elk.terminateWorker();
      givenUp = engine = mainThread("the layout worker stopped answering");
    }
    return givenUp;
  };
  return {
    layout: (graph) => {
      if (givenUp) return givenUp.then((e) => e.layout(graph));
      const work = elk.layout(graph);
      let slow: ReturnType<typeof setTimeout> | undefined;
      const late = new Promise<void>((resolve) => {
        slow = setTimeout(resolve, LAYOUT_MS);
      })
        .then(giveUp)
        .then((e) => e.layout(graph));
      // Whichever answers first: the worker, or the main thread once the worker is given up.
      return Promise.race([work, late]).finally(() => {
        clearTimeout(slow);
      });
    },
  };
}

async function create(): Promise<Engine> {
  if (typeof Worker === "undefined") return mainThread("no Web Worker here");
  if (remembered()) return mainThread("the layout worker didn't start earlier in this session");
  try {
    const elk = await inWorker();
    console.warn("network diagram: ELK runs in a web worker");
    return bounded(elk);
  } catch (e) {
    return mainThread(e instanceof Error ? e.message : String(e));
  }
}

/** The ELK engine, chosen once on first use. */
export function elkEngine(): Promise<Engine> {
  return (engine ??= create());
}

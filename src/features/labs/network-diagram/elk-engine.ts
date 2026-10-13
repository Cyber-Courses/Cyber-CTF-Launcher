import type { ELK } from "elkjs/lib/elk-api";

// ELK (~1.4 MB) is loaded on the first layout, not with the app. In a browser it runs in a Web
// Worker (elk.worker.ts, a same-origin script the CSP allows), off the main thread. Without
// workers (tests), or if the worker can't start, it runs on the main thread from the bundled build.

let engine: Promise<ELK> | undefined;

/** How long the worker gets to answer its first message (loading ELK into it) before the layout
 *  falls back to the main thread. */
const WORKER_START_MS = 15_000;

/** ELK in a worker, once the worker answers (it rejects if the worker fails to load). */
async function inWorker(): Promise<ELK> {
  const { default: ElkApi } = await import("elkjs/lib/elk-api.js");
  let fail: (e: unknown) => void = () => {};
  const broken = new Promise<never>((_, reject) => (fail = reject));
  const slow = setTimeout(() => fail("the layout worker did not answer"), WORKER_START_MS);
  broken.catch(() => {}); // a later error, once it answered, is nobody's to handle
  const elk = new ElkApi({
    workerFactory: () => {
      const worker = new Worker(new URL("./elk.worker.ts", import.meta.url));
      worker.addEventListener("error", (e) => fail(e.message || "the layout worker failed to load"));
      return worker;
    },
  });
  try {
    await Promise.race([elk.knownLayoutAlgorithms(), broken]);
  } catch (e) {
    elk.terminateWorker();
    throw e;
  } finally {
    clearTimeout(slow);
  }
  return elk;
}

async function inline(): Promise<ELK> {
  const { default: Elk } = await import("elkjs/lib/elk.bundled.js");
  return new Elk();
}

async function create(): Promise<ELK> {
  if (typeof Worker === "undefined") return inline();
  try {
    return await inWorker();
  } catch (e) {
    console.warn("network diagram: layout worker unavailable, laying out on the main thread", e);
    return inline();
  }
}

/** The ELK instance, created once on first use. */
export function elkEngine(): Promise<ELK> {
  return (engine ??= create());
}

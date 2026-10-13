// The ELK layout engine, in a Web Worker so a large lab's layout doesn't block the page (see
// elk-engine.ts). elk-worker.js answers the main thread's `elk-api` messages on `self`.
import "elkjs/lib/elk-worker.min.js";

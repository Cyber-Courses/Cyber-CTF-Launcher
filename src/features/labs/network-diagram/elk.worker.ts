// The ELK layout engine, in a Web Worker so a large lab's layout doesn't block the page (see
// elk-engine.ts). elk-worker.js answers the main thread's `elk-api` messages on `self`; once it
// has loaded, the worker says so (elk-api ignores a message without an id).
import "elkjs/lib/elk-worker.min.js";
import { READY } from "@/features/labs/network-diagram/elk-ready";

self.postMessage(READY);

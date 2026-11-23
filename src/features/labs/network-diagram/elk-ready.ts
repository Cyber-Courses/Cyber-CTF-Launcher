/** What the layout worker posts once its script has loaded (elk.worker.ts). */
export const READY = { cmd: "cyberctf-elk-ready" } as const;

export const isReady = (data: unknown) => (data as { cmd?: unknown } | null)?.cmd === READY.cmd;

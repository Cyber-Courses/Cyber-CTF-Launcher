import { describe, expect, it } from "vitest";
import { startOnce } from "@/lib/start-once";

describe("startOnce", () => {
  it("runs one start per key, and a caller that joins sees its log so far and the rest", async () => {
    let log!: (line: string) => void;
    let finish!: () => void;
    let runs = 0;
    const start = (l: (line: string) => void) => {
      runs++;
      log = l;
      return new Promise<void>((r) => (finish = r));
    };
    const first: string[] = [];
    const joined: string[] = [];
    const a = startOnce("k", start, (l) => first.push(l));
    log("booting");
    const b = startOnce("k", start, (l) => joined.push(l));
    log("ready");
    expect(runs).toBe(1);
    expect(first).toEqual(["booting", "ready"]);
    expect(joined).toEqual(["booting", "ready"]);
    finish();
    await Promise.all([a, b]);
    // Done: the next start runs again.
    void startOnce("k", start);
    expect(runs).toBe(2);
    finish();
  });
});

import { warn } from "@/lib/failure";
import { readStored, writeStored } from "@/lib/local-store";

// ---------- last VM-test result, remembered per host (like the machine page) ----------

export type VmTest = { result: "ok" | "fail"; at: number };

/** The last VM test of each host by id (none for a host never tested here). */
export type VmTests = Partial<Record<string, VmTest>>;

const VMTEST_KEY = "cyberctf.server.vmtest";

const isVmTest = (v: unknown): v is VmTest =>
  typeof v === "object" && v !== null && "result" in v && (v.result === "ok" || v.result === "fail") && "at" in v && typeof v.at === "number";

/** The results stored as JSON, keeping only well-formed entries (the storage is the player's). */
function parseVmTests(json: string): VmTests {
  const stored: unknown = JSON.parse(json);
  if (typeof stored !== "object" || stored === null) return {};
  return Object.fromEntries(Object.entries(stored).filter((entry): entry is [string, VmTest] => isVmTest(entry[1])));
}

/** The results remembered on this computer: none when there are none, or storage can't be read
 *  (logged), or what it holds isn't JSON (logged). */
export function loadVmTests(): VmTests {
  const stored = readStored(VMTEST_KEY);
  if (stored === null) return {};
  try {
    return parseVmTests(stored);
  } catch (e) {
    return warn("reading the remembered VM tests", (): VmTests => ({}))(e);
  }
}

/** Remembers `result` as host `id`'s last VM test; storage that refuses is logged, and the
 *  result then lasts as long as the returned list. */
export function saveVmTest(id: string, result: "ok" | "fail"): VmTests {
  const all = { ...loadVmTests(), [id]: { result, at: Date.now() } };
  writeStored(VMTEST_KEY, JSON.stringify(all));
  return all;
}

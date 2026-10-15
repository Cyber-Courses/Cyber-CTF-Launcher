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

export function loadVmTests(): VmTests {
  try {
    return parseVmTests(localStorage.getItem(VMTEST_KEY) ?? "{}");
  } catch {
    return {};
  }
}

export function saveVmTest(id: string, result: "ok" | "fail"): VmTests {
  const all = { ...loadVmTests(), [id]: { result, at: Date.now() } };
  try {
    localStorage.setItem(VMTEST_KEY, JSON.stringify(all));
  } catch {
    /* private window / blocked storage: the result just isn't remembered */
  }
  return all;
}

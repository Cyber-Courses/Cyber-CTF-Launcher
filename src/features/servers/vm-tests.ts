import { changeSettings, settingsNow } from "@/lib/settings-store";

// ---------- last VM-test result, remembered per host (like the machine page) ----------

export type VmTest = { result: "ok" | "fail"; at: number };

/** The last VM test of each host by id (none for a host never tested here). */
export type VmTests = Partial<Record<string, VmTest>>;

/** The results the core keeps with the settings. */
export function loadVmTests(): VmTests {
  return Object.fromEntries(Object.entries(settingsNow().serverTests).map(([id, t]) => [id, { result: t.result, at: t.at }]));
}

/** Remembers `result` as host `id`'s last VM test; answers every host's. */
export function saveVmTest(id: string, result: "ok" | "fail"): VmTests {
  changeSettings({ setting: "serverTest", value: { host: id, record: { result, at: Date.now(), why: null } } });
  return loadVmTests();
}

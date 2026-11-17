import { listen } from "@tauri-apps/api/event";
import { ignore, warn } from "@/lib/failure-log";
import { registerReset } from "@/lib/module-state";
import { settingsChange, settingsGet, type Change, type Settings } from "@/lib/tauri/settings";
import { EVENTS } from "@/lib/tauri/generated/Events";
import { SETTINGS_DEFAULTS } from "@/lib/tauri/generated/SettingsDefaults";

// The player's settings, as the core keeps them (src-tauri/src/settings.rs): this window's copy,
// read synchronously. It starts at the core's defaults, is filled once per window
// (`loadSettings`, which the pages await before they render: `SettingsGate`) and follows every
// change, this window's at once and another window's through the core's `settings-changed`
// event. A change goes to the core, which answers the settings after it; one the core couldn't
// save is logged and lasts as long as this window, as one localStorage refused did before.

let current: Settings = SETTINGS_DEFAULTS;
const listeners = new Set<() => void>();
/** The changes sent and not answered yet: until they are, another window's event isn't taken
 *  over them (it would undo this window's change for a moment). */
let pending = 0;
/** The latest change sent: only its answer replaces the copy (an earlier answer is older). */
let sent = 0;
/** Bumped when the copy is put back to its first value (between tests): an answer to a request
 *  made before is dropped. */
let epoch = 0;
let loading: Promise<void> | null = null;

/** The settings now. */
export const settingsNow = (): Settings => current;

/** Calls `listener` after each change of the settings; answers how to stop. */
export function subscribeSettings(listener: () => void): () => void {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}

/** Takes `next` as the settings: the core's answer, or a test's starting point. */
export function replace(next: Settings) {
  current = next;
  for (const listener of listeners) listener();
}

/** `settings` with `change` applied as it reads here until the core answers (the core applies
 *  its own rules too: an earlier default image isn't kept, a server is no hypervisor). */
export function applyChange(settings: Settings, change: Change): Settings {
  switch (change.setting) {
    case "selfTest":
      return { ...settings, selfTests: { ...settings.selfTests, [change.value.key]: change.value.record } };
    case "serverTest":
      return { ...settings, serverTests: { ...settings.serverTests, [change.value.host]: change.value.record } };
    case "lastRun": {
      const { lab } = change.value;
      const others = Object.fromEntries(Object.entries(settings.lastRuns).filter(([id]) => id !== lab));
      return { ...settings, lastRuns: change.value.at === null ? others : { ...others, [change.value.lab]: change.value.at } };
    }
    case "attackImage":
      return { ...settings, attackImage: change.value?.trim() || null };
    case "attackAuto":
      return { ...settings, attackAuto: change.value };
    case "attackDesktop":
      return { ...settings, attackDesktop: change.value };
    case "vmProvider":
      return { ...settings, vmProvider: change.value };
    case "dockerEngine":
      return { ...settings, dockerEngine: change.value };
    case "portMode":
      return { ...settings, portMode: change.value };
    case "vmForm":
      return { ...settings, vmForm: change.value };
  }
}

/** Applies `changes` here at once, then has the core keep them. */
export function changeSettings(...changes: Change[]) {
  replace(changes.reduce(applyChange, current));
  const mine = ++sent;
  const at = epoch;
  pending++;
  settingsChange(changes)
    .then((after) => {
      if (mine === sent && at === epoch) replace(after);
    })
    .catch(warn("saving a setting"))
    .finally(() => {
      if (at === epoch) pending--;
    });
}

/** Fills this window's copy, once, and follows other windows' changes from then on. The copy
 *  keeps the defaults when the core can't answer (logged). */
export function loadSettings(): Promise<void> {
  const at = epoch;
  loading ??= (async () => {
    listen<Settings>(EVENTS.settingsChanged, (e) => {
      if (pending === 0 && at === epoch) replace(e.payload);
    }).catch(ignore("no events outside the app"));
    const settings = await settingsGet();
    if (at === epoch) replace(settings);
  })().catch(warn("reading the settings"));
  return loading;
}

registerReset(() => {
  current = SETTINGS_DEFAULTS;
  listeners.clear();
  pending = 0;
  sent = 0;
  epoch++;
  loading = null;
});

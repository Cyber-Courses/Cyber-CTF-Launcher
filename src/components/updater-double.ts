// The updater plugin as the update banner sees it, for tests that mock
// `@tauri-apps/plugin-updater` with this module: `check()` answers what `updater.pending` holds,
// or fails while `updater.fail` is set.

/** An update the check found: its version and the install the banner runs. */
export type PendingUpdate = { version: string; downloadAndInstall: () => Promise<void> };

/** What the next check answers. */
export const updater: { pending: PendingUpdate | null; fail: boolean } = { pending: null, fail: false };

/** The plugin's `check`, answered from `updater`. */
export const check = (): Promise<PendingUpdate | null> => (updater.fail ? Promise.reject(new Error("offline")) : Promise.resolve(updater.pending));

/** No update and no failure: each test starts from here. */
export function resetUpdater() {
  updater.pending = null;
  updater.fail = false;
}

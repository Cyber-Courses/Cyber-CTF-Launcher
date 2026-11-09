/** How often a setup window checks the machine again on its own while its last check answered. */
const SETTLED_MS = 60_000;
/** How soon it tries again after a check failed (a daemon starting, a tool busy): the player is
 *  waiting on it. */
const FAILING_MS = 5_000;

/**
 * How often a window that walks the player through setting the machine up (the guided setup, the
 * onboarding) checks the machine again on its own. The check starts a process per tool (on
 * Windows `vagrant` is a Ruby start of a second or two, and the WSL probe more), so it is read
 * when the window opens, when it comes back to the front (an installer finished in its own
 * window), after an install the app ran, and otherwise only this often: a minute, or a few
 * seconds while the last check failed.
 */
export function machineCheckInterval(failing: boolean): number {
  return failing ? FAILING_MS : SETTLED_MS;
}

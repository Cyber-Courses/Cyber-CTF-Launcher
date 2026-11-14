/** The commands a test invoked that the Rust core doesn't register (`installTauri` records
 *  them): every test fails on one (src/test/setup.ts), as the app would. */
const invoked: string[] = [];

/** Records an uncallable command. */
export function recordUncallable(cmd: string): void {
  invoked.push(cmd);
}

/** The uncallable commands invoked since the last call, cleared. */
export function takeUncallable(): string[] {
  return invoked.splice(0);
}

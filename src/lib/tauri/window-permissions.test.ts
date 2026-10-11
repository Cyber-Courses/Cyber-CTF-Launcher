import { describe, expect, it } from "vitest";
import { commandsFrom, permissionSets } from "@/test/command-graph";

// Each window may call only the commands its permission set allows (src-tauri/permissions/
// windows.toml, granted by src-tauri/capabilities/*.json). These are the files each window
// loads; a command one of them invokes but its set doesn't allow would fail at runtime.
const LAYOUT = "app/layout.tsx";
const WINDOWS: Record<string, string[]> = {
  "main-window": [LAYOUT, "app/page.tsx"],
  // Settings loads the main entry with ?window=settings: the app shell runs its hooks, then
  // renders only the settings screen.
  "settings-window": [
    LAYOUT,
    "components/shell/settings-window.tsx",
    "components/shell/use-system-check.ts",
    "components/shell/use-auth-session.ts",
    "components/shell/use-sidebar-labs.ts",
    "lib/deploy-store.ts",
    "lib/appearance.ts",
    "lib/deep-link.ts",
    "features/labs/attack-box-auto.ts",
    "lib/use-tauri-event.ts",
    "lib/app-events.ts",
  ],
  "server-setup-window": [LAYOUT, "app/server-setup/page.tsx"],
  "machine-setup-window": [LAYOUT, "app/machine-setup/page.tsx"],
  "shell-window": [LAYOUT, "app/shell/page.tsx"],
};

const sorted = (s: Set<string> | undefined) => [...(s ?? [])].sort();

describe("window permissions", () => {
  const sets = permissionSets();

  it("has a permission set for every kind of window", () => {
    expect(sorted(new Set(sets.keys()))).toEqual(Object.keys(WINDOWS).sort());
  });

  it.each(Object.entries(WINDOWS))("%s allows exactly the commands its pages invoke", (set, entries) => {
    expect(sorted(sets.get(set))).toEqual(sorted(commandsFrom(entries)));
  });

  it("keeps the terminal's input and output to the terminal windows", () => {
    const pty = ["terminal_open", "terminal_write", "terminal_resize", "terminal_close"];
    for (const [set, allowed] of sets) {
      if (set !== "shell-window") expect(pty.filter((c) => allowed.has(c))).toEqual([]);
    }
  });
});

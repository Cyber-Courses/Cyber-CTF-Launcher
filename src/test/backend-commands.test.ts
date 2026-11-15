import { describe, expect, it } from "vitest";
import { MOCK_KNOBS, mockedCommands } from "@/lib/dev-mock/sample";
import { backendCommands, callable } from "@/test/backend-commands";
import { DEFAULTS } from "@/test/tauri";

// The browser preview and the component tests answer commands as the Rust core would: each one
// they answer must be one it registers, or a renamed or removed command keeps a mock alive for
// a call the app can no longer make.
describe("the doubles answer only what the Rust core registers", () => {
  const registered = backendCommands();

  it("reads the core's command list", () => {
    expect(registered.size).toBeGreaterThan(50);
    expect(registered.has("lab_status")).toBe(true);
  });

  it("the dev mock answers no command the core lacks", () => {
    expect(mockedCommands().filter((c) => !callable(registered, c))).toEqual([]);
  });

  it("the dev mock's knobs are no command the core registers", () => {
    expect([...MOCK_KNOBS].filter((c) => callable(registered, c))).toEqual([]);
  });

  it("the component tests' defaults answer no command the core lacks", () => {
    expect(Object.keys(DEFAULTS).filter((c) => !callable(registered, c))).toEqual([]);
  });
});

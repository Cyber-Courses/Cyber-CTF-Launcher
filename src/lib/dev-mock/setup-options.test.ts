import { describe, expect, it } from "vitest";
import { setupOptions } from "@/lib/dev-mock/setup-options";
import { SETUP_SHAPES } from "@/lib/tauri/generated/SetupShapes";

// The preview's and the tests' stand-in for the core's setup options must give the core's own
// answer (each shape's `setup`, worked out by Rust's `machine::setup::options`): otherwise the
// window's tests would hold it to a plan the core never makes.
describe("the setup options' stand-in", () => {
  it.each(Object.entries(SETUP_SHAPES))("gives the core's options on %s", (name, shape) => {
    expect(setupOptions(shape, name === "windowsHypervWaitsForRestart")).toEqual(shape.setup);
  });
});

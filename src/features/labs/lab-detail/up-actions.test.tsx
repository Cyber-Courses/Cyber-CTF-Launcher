// @vitest-environment jsdom
import { render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { RunningActions } from "@/features/labs/lab-detail/up-actions";
import type { LabState } from "@/features/labs/lab-detail/lab-state";

// Only what the buttons read: a lab up here that can be paused and shut down.
const up = { canPause: true, canShutdown: true, operation: null } as unknown as LabState;

const draw = (parkLocked: boolean) =>
  render(<RunningActions state={up} url={null} locked={false} parkLocked={parkLocked} acting={null} resetting={false} onAct={vi.fn()} onRemove={vi.fn()} />);

describe("RunningActions", () => {
  it("holds Pause and Shut down while an attack box is added, and keeps Stop & remove", () => {
    draw(true);
    expect(screen.getByRole("button", { name: /pause/i })).toHaveProperty("disabled", true);
    expect(screen.getByRole("button", { name: /shut down/i })).toHaveProperty("disabled", true);
    expect(screen.getByRole("button", { name: /stop & remove/i })).toHaveProperty("disabled", false);
  });

  it("offers Pause and Shut down once the box is done", () => {
    draw(false);
    expect(screen.getByRole("button", { name: /pause/i })).toHaveProperty("disabled", false);
    expect(screen.getByRole("button", { name: /shut down/i })).toHaveProperty("disabled", false);
  });
});

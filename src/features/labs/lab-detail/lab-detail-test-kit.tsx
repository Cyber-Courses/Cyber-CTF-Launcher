// The lab page as its tests render it. Not part of the app: only tests import it.

import { render } from "@testing-library/react";
import { vi } from "vitest";
import { LabDetail } from "@/features/labs/lab-detail";
import type { Lab } from "@/features/labs/use-labs";

/** The lab page's handlers, each a spy. */
const pageHandlers = () => ({
  onBack: vi.fn(),
  onStart: vi.fn(),
  onStop: vi.fn(),
  onPark: vi.fn(),
  onResume: vi.fn(),
  onProvision: vi.fn(),
  onLogin: vi.fn(),
});

/** The page of `lab` for a signed-in player on an x86_64 machine, Docker running and VirtualBox
 *  ready, `props` over it: its handlers, to assert on (unless `props` brings its own). */
export function renderLabPage(lab: Lab, props: Partial<Parameters<typeof LabDetail>[0]> = {}) {
  const handlers = pageHandlers();
  render(<LabDetail lab={lab} busy={false} logs={[]} times={[]} loggedIn hostArch="x86_64" readyVms={["virtualbox"]} dockerRunning {...handlers} {...props} />);
  return handlers;
}

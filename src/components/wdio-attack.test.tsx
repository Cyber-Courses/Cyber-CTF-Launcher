// @vitest-environment jsdom
import { act, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { WDIO_ATTACK_EVENT, WdioAttack } from "@/components/wdio-attack";

vi.mock("@/features/labs/attack-box-panel", () => ({ AttackBoxPanel: () => <p>attack box panel</p> }));
vi.mock("@/features/labs/use-attack-box", () => ({ useAttackBox: (labId: string) => ({ labId, status: null }) }));
vi.mock("@/features/labs/lab-detail/use-lab-shell", () => ({ useLabShell: () => ({ open: () => {} }) }));

afterEach(() => {
  localStorage.clear();
});

describe("the e2e attack-box panel", () => {
  // #321: the specs reloaded the page to mount it, the reload that ends the Windows app.
  it("mounts for the lab a spec names, told by an event, with no reload", () => {
    const { container } = render(<WdioAttack />);
    expect(container.querySelector("[data-wdio-attack]")).toBeNull();
    act(() => {
      localStorage.setItem("__wdio_attack__", "local-web");
      window.dispatchEvent(new Event(WDIO_ATTACK_EVENT));
    });
    expect(screen.getByText("attack box panel")).toBeTruthy();
    expect(container.querySelector("[data-wdio-attack]")).not.toBeNull();
  });

  it("mounts at once for a lab named before the page loaded", () => {
    localStorage.setItem("__wdio_attack__", "local-web");
    render(<WdioAttack />);
    expect(screen.getByText("attack box panel")).toBeTruthy();
  });
});

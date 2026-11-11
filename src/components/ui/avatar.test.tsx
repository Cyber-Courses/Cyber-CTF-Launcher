// @vitest-environment jsdom
import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { AvatarGroup } from "@/components/ui/avatar";

const names = ["Docker Desktop", "OrbStack", "Colima", "VirtualBox", "VMware Fusion", "UTM", "Parallels"];

describe("AvatarGroup", () => {
  it("shows five, then the rest as one +N avatar that names them", () => {
    render(<AvatarGroup items={names.map((name) => ({ name }))} />);
    const avatars = screen.getAllByRole("img");
    expect(avatars.map((a) => a.getAttribute("aria-label"))).toEqual([...names.slice(0, 5), "2 more: UTM, Parallels"]);
    expect(avatars.at(-1)?.textContent).toBe("+2");
    expect(screen.getAllByRole("tooltip", { hidden: true }).at(-1)?.textContent).toBe("UTM, Parallels");
  });

  it("shows every one, with no count, up to the maximum", () => {
    render(<AvatarGroup items={names.slice(0, 5).map((name) => ({ name }))} />);
    expect(screen.getAllByRole("img")).toHaveLength(5);
    expect(document.body.textContent).not.toMatch(/\+\d/);
  });

  it("falls back to its initials (given, else from its name) on the muted background, and keeps each tooltip out of the reading", () => {
    render(
      <AvatarGroup
        items={[
          { name: "VMware ESXi", initials: "ES" },
          { name: "Hyper-V" },
          { name: "Parallels" },
          { name: "OrbStack", src: "/brands/orbstack.png", fill: true },
        ]}
      />,
    );
    const avatars = screen.getAllByRole("img");
    expect(avatars.map((a) => a.textContent)).toEqual(["ES", "HV", "P", ""]);
    expect(avatars[0].firstElementChild?.className).toContain("bg-muted");
    // The app icon fills the circle; a ring in the background's colour around each.
    expect(avatars[3].querySelector("img")?.className).toContain("size-full rounded-full");
    expect(avatars[3].className).toContain("ring-background");
    for (const tip of screen.getAllByRole("tooltip", { hidden: true })) expect(tip.getAttribute("aria-hidden")).toBe("true");
  });
});

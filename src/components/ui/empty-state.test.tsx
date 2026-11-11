// @vitest-environment jsdom
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { Button } from "@/components/ui/button";
import { EmptyState } from "@/components/ui/empty-state";
import { LogoTile } from "@/components/ui/logo-tile";

describe("EmptyState", () => {
  it("is a region named by its title, an h3, with its one sentence", () => {
    render(<EmptyState icon="server" title="No server connected" description="Add a server to run labs on it." />);
    const region = screen.getByRole("region", { name: "No server connected" });
    expect(screen.getByRole("heading", { level: 3, name: "No server connected" }).closest("section")).toBe(region);
    expect(region.textContent).toContain("Add a server to run labs on it.");
    // The icon tile is decoration: nothing for a screen reader, no stray name on the region.
    expect(region.querySelector("svg")?.getAttribute("aria-hidden")).toBe("true");
  });

  it("puts the action before the secondary link, in reading and focus order", async () => {
    const add = vi.fn();
    const other = vi.fn();
    const user = userEvent.setup();
    render(
      <EmptyState
        icon="machine"
        title="No provider installed"
        description="Install one to run labs here."
        action={<Button onClick={add}>Add a provider</Button>}
        secondary={
          <Button variant="link" onClick={other}>
            Use a server
          </Button>
        }
      />,
    );
    expect(screen.getAllByRole("button").map((b) => b.textContent)).toEqual(["Add a provider", "Use a server"]);
    await user.tab();
    expect(document.activeElement?.textContent).toBe("Add a provider");
    await user.keyboard("{Enter}");
    expect(add).toHaveBeenCalledOnce();
    await user.tab();
    expect(document.activeElement?.textContent).toBe("Use a server");
    await user.keyboard("{Enter}");
    expect(other).toHaveBeenCalledOnce();
  });

  it("shows its marks between the sentence and the action, hidden from assistive tech when they are decoration", () => {
    render(
      <EmptyState
        icon="server"
        title="No server connected"
        description="Add a server to run labs on it."
        marks={[
          { name: "Proxmox", logo: <LogoTile src="/brands/proxmox-icon.svg" size="sm" /> },
          { name: "VMware ESXi", logo: <LogoTile size="sm" /> },
        ]}
        action={<Button>Add server</Button>}
      />,
    );
    const region = screen.getByRole("region", { name: "No server connected" });
    const row = region.querySelector("ul");
    expect(row?.getAttribute("aria-hidden")).toBe("true");
    expect(row?.textContent).toBe("ProxmoxVMware ESXi");
    expect([...region.children].map((c) => c.tagName)).toEqual(["DIV", "H3", "P", "UL", "DIV"]);
    // Decoration: no list, no image the screen reader would announce; the one button is the action.
    expect(screen.queryByRole("list")).toBeNull();
    expect(screen.getAllByRole("button").map((b) => b.textContent)).toEqual(["Add server"]);
  });

  it("makes marks with a shortcut buttons named by their label, before the action", async () => {
    const aws = vi.fn();
    const user = userEvent.setup();
    render(
      <EmptyState
        icon="cloud"
        title="No cloud account connected"
        marks={[{ name: "AWS", logo: <LogoTile src="/brands/aws.svg" size="sm" />, select: { label: "Connect AWS", onSelect: aws } }]}
        action={<Button>Set up cloud provider</Button>}
      />,
    );
    expect(screen.getByRole("list").getAttribute("aria-hidden")).toBeNull();
    expect(screen.getAllByRole("button").map((b) => b.getAttribute("aria-label") ?? b.textContent)).toEqual(["Connect AWS", "Set up cloud provider"]);
    await user.click(screen.getByRole("button", { name: "Connect AWS" }));
    expect(aws).toHaveBeenCalledOnce();
  });

  it("leaves out what it isn't given: no description, no actions row", () => {
    const { container } = render(<EmptyState icon="labs" title="No labs match" className="mt-2" />);
    const region = screen.getByRole("region", { name: "No labs match" });
    expect(region.className).toContain("mt-2");
    expect(container.querySelectorAll("p")).toHaveLength(0);
    expect(screen.queryAllByRole("button")).toHaveLength(0);
    expect(region.children).toHaveLength(2);
  });
});

// @vitest-environment jsdom
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { Button } from "@/components/ui/button";
import { EmptyState } from "@/components/ui/empty-state";

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

  it("leaves out what it isn't given: no description, no actions row", () => {
    const { container } = render(<EmptyState icon="labs" title="No labs match" className="mt-2" />);
    const region = screen.getByRole("region", { name: "No labs match" });
    expect(region.className).toContain("mt-2");
    expect(container.querySelectorAll("p")).toHaveLength(0);
    expect(screen.queryAllByRole("button")).toHaveLength(0);
    expect(region.children).toHaveLength(2);
  });
});

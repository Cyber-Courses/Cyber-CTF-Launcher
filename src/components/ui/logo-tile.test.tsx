// @vitest-environment jsdom
import { render } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { LogoTile } from "@/components/ui/logo-tile";

describe("LogoTile", () => {
  it("puts a mark on the white logo tile, decorative", () => {
    const { container } = render(<LogoTile src="/brands/proxmox-icon.svg" size="sm" />);
    const tile = container.firstElementChild;
    expect(tile?.className).toContain("logo-tile");
    expect(tile?.className).toContain("size-6");
    expect(tile?.querySelector("img")?.getAttribute("alt")).toBe("");
  });

  it("lets an app icon fill the square itself", () => {
    const { container } = render(<LogoTile src="/brands/orbstack.png" fill />);
    const img = container.firstElementChild;
    expect(img?.tagName).toBe("IMG");
    expect(img?.className).toContain("size-8");
    expect(img?.className).not.toContain("logo-tile");
  });

  it("shows a neutral server glyph, not an image, for a provider without a mark", () => {
    const { container } = render(<LogoTile />);
    expect(container.querySelector("img")).toBeNull();
    expect(container.querySelector("svg")?.getAttribute("aria-hidden")).toBe("true");
  });
});

import { isValidElement, type ReactNode } from "react";
import { describe, expect, it, vi } from "vitest";
import { AppearanceSync } from "@/lib/appearance";

// Next's font loaders run only under its compiler.
vi.mock("next/font/local", () => ({ default: () => ({ variable: "font-local" }) }));
vi.mock("geist/font/sans", () => ({ GeistSans: { variable: "font-sans" } }));
vi.mock("geist/font/mono", () => ({ GeistMono: { variable: "font-mono" } }));

const { default: RootLayout } = await import("@/app/layout");

/** Whether `node`'s element tree has an element of `type`. */
function contains(node: ReactNode, type: unknown): boolean {
  if (Array.isArray(node)) return (node as ReactNode[]).some((n) => contains(n, type));
  if (!isValidElement<{ children?: ReactNode }>(node)) return false;
  const children: ReactNode = node.props.children;
  return node.type === type || contains(children, type);
}

describe("the root layout", () => {
  it("keeps every window on the appearance, its system frame's theme included", () => {
    // Regression: only the main and Settings windows synced it, so on Windows the shell and
    // setup windows kept the system's light frame over the dark app.
    const tree = RootLayout({ children: null, params: Promise.resolve({}) });
    expect(contains(tree, AppearanceSync)).toBe(true);
  });
});

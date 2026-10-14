// @vitest-environment jsdom
import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

const { openUrl } = vi.hoisted(() => ({ openUrl: vi.fn(async () => {}) }));
vi.mock("@tauri-apps/plugin-opener", () => ({ openUrl }));

const { Markdown } = await import("@/components/ui/markdown");

const md = (content: string) => render(<Markdown content={content} />).container;

// Write-ups come from lab authors: the renderer must survive whatever Markdown they write.
describe("Markdown: block edges", () => {
  it("a line starting with # that is not a heading is text, and up to six hashes make a heading", () => {
    // "#### Deeper", "#hashtag" and a lone "#" match neither the heading regex (one to three
    // hashes then whitespace) nor any other block, and the paragraph loop refuses lines that
    // start with "#", so `i` never moves and the while loop spins forever: the window freezes
    // on such a write-up. Skipped because it cannot complete (confirmed under a 45 s alarm).
    // Suggested fix: drop the `startsWith("#")` guard from the paragraph loop (test the heading
    // regex there instead), and accept up to six hashes as headings.
    for (const content of ["#### Deeper", "#hashtag", "#"]) {
      const { container, unmount } = render(<Markdown content={content} />);
      expect(container.textContent?.length, content).toBeGreaterThan(0);
      unmount();
    }
  });

  it("an unclosed code fence runs to the end, and a fence with a language keeps its content verbatim", () => {
    const c = md("```bash\necho **not bold**\n```\n\nafter");
    expect(c.querySelector("pre")?.textContent).toBe("echo **not bold**");
    expect(c.querySelector("strong")).toBeNull();
    expect(c.querySelector("p")?.textContent).toBe("after");
    expect(md("```\nopen\nstill").querySelector("pre")?.textContent).toBe("open\nstill");
  });

  it("joins blockquote and paragraph lines with spaces, and reads CRLF line ends", () => {
    const c = md("> a\r\n> b\r\n\r\nc\r\nd");
    expect(c.querySelector("blockquote")?.textContent).toBe("a b");
    expect(c.querySelector("p")?.textContent).toBe("c d");
  });

  it("draws a rule for --- and ***, but not for --", () => {
    expect(md("---\n\n***").querySelectorAll("hr")).toHaveLength(2);
    expect(md("--").querySelector("hr")).toBeNull();
    expect(md("--").querySelector("p")?.textContent).toBe("--");
  });

  it("a heading can hold code and a link, one level under the page title", () => {
    const c = md("## See `x` and [y](https://e.com)");
    const h = c.querySelector("h3");
    expect(h?.querySelector("code")?.textContent).toBe("x");
    expect(h?.querySelector("a")?.getAttribute("href")).toBe("https://e.com");
  });

  it("lists mix markers, number themselves, and hold inline markup", () => {
    const c = md("- a\n* **b**\n\n1. one\n2. two");
    expect(c.querySelectorAll("ul > li")).toHaveLength(2);
    expect(c.querySelector("ul strong")?.textContent).toBe("b");
    expect(c.querySelectorAll("ol > li")).toHaveLength(2);
  });

  it("renders nothing for empty or blank content", () => {
    expect(md("").childNodes[0].childNodes).toHaveLength(0);
    expect(md("\n\n  \n").childNodes[0].childNodes).toHaveLength(0);
  });

  it("renders a very long line as one paragraph", () => {
    const c = md("a".repeat(200_000) + " **b** " + "c".repeat(200_000));
    expect(c.querySelectorAll("p")).toHaveLength(1);
    expect(c.querySelector("strong")?.textContent).toBe("b");
  });
});

describe("Markdown: inline edges", () => {
  it("nested emphasis renders the inner bold and keeps the outer asterisks as text", () => {
    const c = md("***x***");
    expect(c.querySelector("strong")?.textContent).toBe("x");
    expect(c.textContent).toBe("*x*");
  });

  it("inline code wins over emphasis inside it", () => {
    const c = md("`*not italic*`");
    expect(c.querySelector("code")?.textContent).toBe("*not italic*");
    expect(c.querySelector("em")).toBeNull();
  });

  it("leaves an unclosed link or bracket as plain text", () => {
    for (const text of ["[text](url", "[text", "](x)", "[a] (b)"]) {
      const c = md(text);
      expect(c.querySelector("a"), text).toBeNull();
      expect(c.textContent, text).toBe(text);
    }
  });

  it("a link whose URL holds parentheses keeps them", () => {
    // `\([^)]+\)` stops at the first ")", so a Wikipedia-style URL loses its tail and the
    // stray ")" shows as text after the link. Suggested fix: allow one level of balanced
    // parentheses in the URL part, as CommonMark does: `\(((?:[^()]|\([^()]*\))+)\)`.
    const c = md("[wiki](https://en.wikipedia.org/wiki/Foo_(bar))");
    expect(c.querySelector("a")?.getAttribute("href")).toBe("https://en.wikipedia.org/wiki/Foo_(bar)");
    expect(c.textContent).toBe("wiki");
  });

  it("a javascript: link is plain text and never reaches the system opener", () => {
    // Write-ups are third-party content: a `javascript:` (or `data:`) URL should not become a
    // link, let alone be passed to openUrl. The opener plugin's scope is the only thing stopping
    // it today. Suggested fix: render the text alone unless the URL starts with http(s)://,
    // mailto: or a relative path.
    md("[click](javascript:alert(1))");
    const link = screen.queryByRole("link");
    if (link) fireEvent.click(link);
    expect(openUrl).not.toHaveBeenCalledWith(expect.stringMatching(/^javascript:/));
    expect(link?.getAttribute("href") ?? "").not.toMatch(/^javascript:/);
  });

  it("a link click opens the browser instead of navigating", () => {
    md("[docs](https://example.com/docs)");
    const link = screen.getByRole("link");
    const event = fireEvent.click(link);
    expect(event).toBe(false);
    expect(openUrl).toHaveBeenCalledWith("https://example.com/docs");
  });
});

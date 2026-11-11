import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterEach, describe, expect, it } from "vitest";
import { anchors, brokenLinks, links, slug } from "./doc-links.ts";

describe("check/doc-links", () => {
  it("makes a heading's anchor the way GitHub does", () => {
    expect(slug("Proxy and custom certificate authorities")).toBe("proxy-and-custom-certificate-authorities");
    expect(slug("Privacy and what is sent")).toBe("privacy-and-what-is-sent");
    expect(slug("The `cyberctf://` scheme, v1.2")).toBe("the-cyberctf-scheme-v12");
    // GitHub keeps an underscore inside a word; emphasis marks go.
    expect(slug("The `lab_dir` folder")).toBe("the-lab_dir-folder");
    expect(slug("An _emphasised_ word")).toBe("an-emphasised-word");
  });

  it("numbers repeated headings and skips fenced code", () => {
    const md = "# Windows\n\n## Windows\n\n```bash\n# not a heading\n```\n";
    expect([...anchors(md)]).toEqual(["windows", "windows-1"]);
  });

  it("finds inline links, images and reference definitions, not code", () => {
    const md = "See [a](a.md#x) and ![b](img.png).\n`[c](c.md)`\n\n[d]: https://example.com\n\n```\n[e](e.md)\n```\n";
    expect(links(md)).toEqual(["a.md#x", "img.png", "https://example.com"]);
  });

  describe("brokenLinks", () => {
    let dir = "";
    afterEach(() => {
      rmSync(dir, { recursive: true, force: true });
    });

    it("passes a file, a folder and a heading that exist, and skips a link with a scheme", () => {
      dir = mkdtempSync(join(tmpdir(), "doc-links-"));
      mkdirSync(join(dir, "guide"));
      writeFileSync(join(dir, "other.md"), "# Set up\n");
      const page = join(dir, "page.md");
      const md = "[a](other.md) [b](other.md#set-up) [c](guide) [d](#here) [e](https://nowhere.invalid) [f](mailto:x@y.z)\n\n# Here\n";
      writeFileSync(page, md);
      expect(brokenLinks(page, md)).toEqual([]);
    });

    it("names a missing file and a heading the page doesn't have", () => {
      dir = mkdtempSync(join(tmpdir(), "doc-links-"));
      writeFileSync(join(dir, "other.md"), "# Set up\n");
      const page = join(dir, "page.md");
      const md = "[a](gone.md) [b](other.md#tear-down) [c](#nowhere)\n";
      writeFileSync(page, md);
      expect(brokenLinks(page, md)).toEqual([
        { link: "gone.md", why: "no such file" },
        { link: "other.md#tear-down", why: "no heading #tear-down" },
        { link: "#nowhere", why: "no heading #nowhere" },
      ]);
    });
  });
});

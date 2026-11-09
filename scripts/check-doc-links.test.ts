import { describe, expect, it } from "vitest";
import { anchors, links, slug } from "./check-doc-links.ts";

describe("check-doc-links", () => {
  it("makes a heading's anchor the way GitHub does", () => {
    expect(slug("Proxy and custom certificate authorities")).toBe("proxy-and-custom-certificate-authorities");
    expect(slug("Privacy and what is sent")).toBe("privacy-and-what-is-sent");
    expect(slug("The `cyberctf://` scheme, v1.2")).toBe("the-cyberctf-scheme-v12");
  });

  it("numbers repeated headings and skips fenced code", () => {
    const md = "# Windows\n\n## Windows\n\n```bash\n# not a heading\n```\n";
    expect([...anchors(md)]).toEqual(["windows", "windows-1"]);
  });

  it("finds inline links, images and reference definitions, not code", () => {
    const md = "See [a](a.md#x) and ![b](img.png).\n`[c](c.md)`\n\n[d]: https://example.com\n\n```\n[e](e.md)\n```\n";
    expect(links(md)).toEqual(["a.md#x", "img.png", "https://example.com"]);
  });
});

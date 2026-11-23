import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";
import { nextDevArgs, previewEnv } from "./web.ts";

describe("pnpm dev:web", () => {
  it("runs next dev on the loopback interface, on its own port unless told another", () => {
    expect(nextDevArgs([])).toEqual(["dev", "--hostname", "127.0.0.1", "--port", "3207"]);
    expect(nextDevArgs(["--port", "3210"])).toEqual(["dev", "--hostname", "127.0.0.1", "--port", "3210"]);
    expect(nextDevArgs(["-p", "3211"]).at(-1)).toBe("3211");
    expect(nextDevArgs(["--port=3212"]).at(-1)).toBe("3212");
  });

  it("refuses a hostname, any other argument and a port that isn't one", () => {
    expect(() => nextDevArgs(["--hostname", "0.0.0.0"])).toThrow("takes --port <n> alone, not --hostname");
    expect(() => nextDevArgs(["-H", "0.0.0.0"])).toThrow("not -H");
    expect(() => nextDevArgs(["--port"])).toThrow("not --port");
    for (const port of ["0", "65536", "web", "80; rm"]) expect(() => nextDevArgs(["--port", port])).toThrow("not a port");
  });

  it("builds the mock in, keeping the caller's environment", () => {
    expect(previewEnv({ PATH: "/bin", NEXT_PUBLIC_DEV_MOCK: "" })).toEqual({ PATH: "/bin", NEXT_PUBLIC_DEV_MOCK: "1" });
  });
});

describe("the app's dev server", () => {
  it("listens on the loopback interface only, at the address the app's window loads (devUrl)", () => {
    const pkg = JSON.parse(readFileSync("package.json", "utf8")) as { scripts: Record<string, string> };
    const conf = JSON.parse(readFileSync("src-tauri/tauri.conf.json", "utf8")) as { build: { devUrl: string } };
    const devUrl = new URL(conf.build.devUrl);
    expect(devUrl.hostname).toBe("127.0.0.1");
    expect(pkg.scripts.dev).toBe(`next dev --hostname 127.0.0.1 --port ${devUrl.port}`);
    expect(Number(devUrl.port)).not.toBe(3207);
  });
});

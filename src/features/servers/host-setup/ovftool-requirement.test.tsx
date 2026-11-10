// @vitest-environment jsdom
import { screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it } from "vitest";
import { EMPTY_HOST } from "@/features/servers/host-setup";
import { argsOf, failsWith, go, renderSetup } from "@/features/servers/host-setup/setup-test-kit";
import type { OvftoolPlaced, SystemReport } from "@/lib/tauri";
import { INSTALLS_OFF, readyReport } from "@/test/fixtures";
import { installTauri, type Call, type Handler } from "@/test/tauri";
import { button, flush } from "@/test/ui";

const FUSION = "https://support.broadcom.com/group/ecx/productdownloads?subfamily=VMware%20Fusion&freeDownloads=true";
const WORKSTATION = "https://support.broadcom.com/group/ecx/productdownloads?subfamily=VMware%20Workstation%20Pro&freeDownloads=true";
const STANDALONE = "https://developer.broadcom.com/tools/open-virtualization-format-ovf-tool/latest";
const PICKED = "/Users/player/Downloads/VMware OVF Tool";

/** This machine with Vagrant and its ESXi plugin, on `os`/`arch`, and the OVF Tool as `ovftool`. */
function esxiReport(os: string, arch: string, ovftool: Partial<Pick<SystemReport, "ovftool" | "ovftoolLocation">> = {}): SystemReport {
  const r = readyReport();
  return {
    ...r,
    os,
    arch,
    ovftool: { installed: false, version: null },
    ovftoolLocation: null,
    vmProviders: r.vmProviders.map((p) => (p.provider === "vmware_esxi" ? { ...p, pluginInstalled: true } : p)),
    ...ovftool,
  };
}

/** The ESXi setup's tools step, on `report`. */
async function toolsStep(report: SystemReport) {
  const user = userEvent.setup();
  const handlers = await renderSetup({ ...EMPTY_HOST, provider: "vmware_esxi" }, report);
  await go(user, /^continue$/i);
  expect(screen.getByText("VMware OVF Tool")).toBeTruthy();
  return { user, handlers };
}

const opened = (calls: Call[]) => argsOf(calls, "plugin:opener|open_url").map((a) => a?.url);

/** The core's answer to the picked folder, and the folder the dialog gives (null: cancelled). */
function placing(answer: OvftoolPlaced | Handler, picked: string | null = PICKED) {
  return installTauri({
    "plugin:dialog|open": () => picked,
    ovftool_place: typeof answer === "function" ? answer : () => answer,
  });
}

describe("the OVF Tool row", () => {
  it("offers VMware Fusion and the standalone download on a Mac, each at Broadcom", async () => {
    const calls = installTauri();
    const { user } = await toolsStep(esxiReport("macos", "aarch64"));
    await go(user, /get vmware fusion \(includes ovf tool\)/i);
    await go(user, /get the standalone ovf tool/i);
    expect(opened(calls)).toEqual([FUSION, STANDALONE]);
    expect(screen.queryByRole("button", { name: /workstation/i })).toBeNull();
  });

  it("offers VMware Workstation on an x64 PC, Windows or Linux", async () => {
    for (const os of ["windows", "linux"]) {
      const calls = installTauri();
      const { user } = await toolsStep(esxiReport(os, "x86_64"));
      await go(user, /get vmware workstation \(includes ovf tool\)/i);
      expect(opened(calls)).toEqual([WORKSTATION]);
      expect(screen.queryByRole("button", { name: /fusion/i })).toBeNull();
      document.body.innerHTML = "";
    }
  });

  it("offers no hypervisor where VMware has none (Linux on Arm): the standalone tool still", async () => {
    installTauri();
    await toolsStep(esxiReport("linux", "aarch64"));
    expect(screen.queryByRole("button", { name: /includes ovf tool/i })).toBeNull();
    expect(button(/get the standalone ovf tool/i)).toBeTruthy();
    expect(button(/use a downloaded ovf tool/i)).toBeTruthy();
  });

  it("places the folder the player picks, says where, and checks the machine again", async () => {
    const path = "/Users/player/Library/Application Support/org.cyberctf.desktop/tools/ovftool/ovftool";
    const calls = placing({ outcome: "placed", path, needsRosetta: false });
    const { user, handlers } = await toolsStep(esxiReport("macos", "aarch64"));
    await go(user, /use a downloaded ovf tool/i);
    expect(argsOf(calls, "plugin:dialog|open")[0]).toMatchObject({ options: { directory: true, multiple: false } });
    expect(argsOf(calls, "ovftool_place")).toEqual([{ folder: PICKED }]);
    expect(screen.getByRole("status").textContent).toBe(`OVF Tool copied into Cyber CTF's tools folder: ${path}`);
    expect(handlers.onRefresh).toHaveBeenCalled();
  });

  it("says Rosetta is needed when the placed tool is built for Intel Macs only", async () => {
    placing({ outcome: "placed", path: "/x/ovftool", needsRosetta: true });
    const { user } = await toolsStep(esxiReport("macos", "aarch64"));
    await go(user, /use a downloaded ovf tool/i);
    expect(screen.getByRole("status").textContent).toContain("softwareupdate --install-rosetta");
  });

  it("says why a folder was refused, naming the team that signed it, and checks nothing again", async () => {
    placing({ outcome: "refused", reason: "notVmware", detail: "ABCDE12345" });
    const { user, handlers } = await toolsStep(esxiReport("macos", "aarch64"));
    await go(user, /use a downloaded ovf tool/i);
    expect(screen.getByRole("status").textContent).toBe("It's signed by team ABCDE12345, not by VMware (EG7KH642X6), so it wasn't used.");
    expect(handlers.onRefresh).not.toHaveBeenCalled();
  });

  it.each([
    ["noTool", null, /no ovftool program in that folder/],
    ["noLibraries", null, /not its lib folder/],
    ["tooLarge", null, /far more than the OVF Tool/],
    ["unsigned", "ovftool: code object is not signed at all", /isn't validly signed \(ovftool: code object is not signed at all\)/],
    ["notVmware", null, /isn't signed by VMware/],
  ] as const)("says the refusal %s", async (reason, detail, says) => {
    placing({ outcome: "refused", reason, detail });
    const { user } = await toolsStep(esxiReport("macos", "aarch64"));
    await go(user, /use a downloaded ovf tool/i);
    expect(screen.getByRole("status").textContent).toMatch(says);
  });

  it("says a failed placement, and asks nothing when the picker is cancelled", async () => {
    placing(failsWith("No space left on device"));
    const { user } = await toolsStep(esxiReport("macos", "aarch64"));
    await go(user, /use a downloaded ovf tool/i);
    expect(screen.getByRole("status").textContent).toBe("No space left on device");
    document.body.innerHTML = "";
    const calls = placing({ outcome: "placed", path: "/x", needsRosetta: false }, null);
    const next = await toolsStep(esxiReport("macos", "aarch64"));
    await go(next.user, /use a downloaded ovf tool/i);
    expect(argsOf(calls, "ovftool_place")).toEqual([]);
    expect(screen.queryByRole("status")).toBeNull();
  });

  it("can't place a tool when the computer's policy turns tool installs off", async () => {
    installTauri(INSTALLS_OFF);
    await toolsStep(esxiReport("macos", "aarch64"));
    await flush(30);
    expect(button(/use a downloaded ovf tool/i).hasAttribute("disabled")).toBe(true);
  });

  it("shows the version found and where, and no ways to get another", async () => {
    installTauri();
    const path = "/Applications/VMware Fusion.app/Contents/Library/VMware OVF Tool/ovftool";
    await toolsStep(
      esxiReport("macos", "aarch64", {
        ovftool: { installed: true, version: "VMware ovftool 5.0.0" },
        ovftoolLocation: { source: "fusion", path, needsRosetta: false },
      }),
    );
    expect(screen.getByText(`VMware ovftool 5.0.0 · VMware Fusion's copy: ${path}`)).toBeTruthy();
    expect(screen.queryByRole("button", { name: /ovf tool/i })).toBeNull();
  });

  it("says the copy found needs Rosetta when it can't run without it", async () => {
    installTauri();
    const path = "/Users/p/Library/Application Support/org.cyberctf.desktop/tools/ovftool/ovftool";
    await toolsStep(esxiReport("macos", "aarch64", { ovftoolLocation: { source: "managed", path, needsRosetta: true } }));
    expect(screen.getByText(new RegExp(`Found at ${path.replace(/\./g, "\\.")}, but it's built for Intel Macs`))).toBeTruthy();
  });
});

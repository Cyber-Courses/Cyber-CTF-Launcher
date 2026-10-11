import { describe, expect, it } from "vitest";

import { setupNeeded } from "@/features/labs/lab-readiness";
import type { Lab } from "@/features/labs/use-labs";
import type { ProviderStatus, SystemReport } from "@/lib/tauri";

const provider = (p: string, remote = false, available = false): ProviderStatus => ({
  provider: p as ProviderStatus["provider"],
  remote,
  available,
  hypervisor: available,
  plugin: null,
  pluginInstalled: available,
  reason: null,
});

const report = (vmProviders: ProviderStatus[]) => ({ docker: { installed: true }, dockerRunning: true, vmProviders }) as unknown as SystemReport;

const vmLab = (providers: string[]) => ({ runtime: { runtime: "VM", providers } }) as unknown as Lab;

describe("setupNeeded", () => {
  it("names the local hypervisors a VM lab supports", () => {
    const r = report([provider("libvirt"), provider("virtualbox"), provider("parallels")]);
    expect(setupNeeded(vmLab(["virtualbox", "parallels", "aws"]), r, [])).toBe("Needs VirtualBox or Parallels, or a server");
  });

  it("does not offer a hypervisor the lab does not support", () => {
    // The report lists every local hypervisor, installed or not; libvirt is not one of this lab's.
    const r = report([provider("libvirt"), provider("virtualbox")]);
    expect(setupNeeded(vmLab(["virtualbox"]), r, [])).toBe("Needs VirtualBox, or a server");
  });

  it("is null when a supported hypervisor is usable", () => {
    const r = report([provider("virtualbox", false, true)]);
    expect(setupNeeded(vmLab(["virtualbox"]), r, [])).toBeNull();
  });

  describe("a VM lab built for another CPU (x86 on Apple Silicon)", () => {
    const arm = (vmProviders: ProviderStatus[]) => ({ ...report(vmProviders), arch: "aarch64", vagrant: { installed: true } }) as unknown as SystemReport;
    const x86Lab = { runtime: { runtime: "VM", providers: ["virtualbox", "vmware_desktop"], architectures: ["x86_64"] } } as unknown as Lab;

    it("needs QEMU, not the hypervisors it lists, which can't run it here", () => {
      const r = arm([provider("virtualbox", false, true), provider("qemu")]);
      expect(setupNeeded(x86Lab, r, [])).toBe("Needs QEMU, or a server");
    });

    it("runs emulated once QEMU is ready", () => {
      expect(setupNeeded(x86Lab, arm([provider("qemu", false, true)]), [])).toBeNull();
    });
  });
});

describe("setupNeeded: container labs and missing facts", () => {
  const dockerLab = { runtime: { runtime: "DOCKER", providers: ["docker"] } } as unknown as Lab;
  const docker = (p: Partial<SystemReport>) => ({ ...report([]), ...p }) as SystemReport;

  it("says what the container engine needs, in order", () => {
    expect(setupNeeded(dockerLab, docker({ docker: { installed: false, version: null } }), [])).toBe("Needs a container engine");
    expect(setupNeeded(dockerLab, docker({ dockerDenied: true, dockerRunning: false }), [])).toBe("Docker needs permission (see Machine)");
    expect(setupNeeded(dockerLab, docker({ dockerRunning: false }), [])).toBe("Start your container engine");
    expect(setupNeeded(dockerLab, docker({}), [])).toBeNull();
  });

  it("is null without a runtime or before the machine report is in", () => {
    expect(setupNeeded({ runtime: null } as unknown as Lab, report([]), [])).toBeNull();
    expect(setupNeeded(dockerLab, null, [])).toBeNull();
  });

  it("asks for a server when no local hypervisor can run the lab, and none when one is saved", () => {
    const lab = vmLab(["vmware_esxi"]);
    expect(setupNeeded(lab, report([provider("virtualbox")]), [])).toBe("Needs a server to run on");
    expect(setupNeeded(lab, report([]), [{ id: "esx", provider: "vmware_esxi" } as never])).toBeNull();
  });
});

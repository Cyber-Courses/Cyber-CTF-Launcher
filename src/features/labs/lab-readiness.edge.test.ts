import { describe, expect, it } from "vitest";

import { joinOr, needsCloudAccount, setupNeeded } from "@/features/labs/lab-readiness";
import { cloudNames } from "@/features/labs/lab-runtime";
import type { Lab } from "@/features/labs/use-labs";
import { translate } from "@/lib/i18n";
import type { ProviderStatus, ServerHost, SystemReport } from "@/lib/tauri";

const provider = (p: string, available = true): ProviderStatus =>
  ({ provider: p, remote: false, available, hypervisor: available, plugin: null, pluginInstalled: available, reason: null }) as ProviderStatus;
const report = (over: Partial<SystemReport> = {}) =>
  ({
    os: "linux",
    arch: "x86_64",
    docker: { installed: true },
    dockerRunning: true,
    terraform: { installed: false },
    vmProviders: [],
    ...over,
  }) as unknown as SystemReport;
const cloudLab = (providers: string[]) => ({ runtime: { runtime: "CLOUD", architectures: [], providers } }) as unknown as Lab;
const vmLab = (providers: string[]) => ({ runtime: { runtime: "VM", architectures: [], providers } }) as unknown as Lab;
const host = (p: string) => ({ id: p, name: p, provider: p }) as unknown as ServerHost;

describe("joinOr", () => {
  it("joins none, one, two and three names", () => {
    expect(joinOr([])).toBe("");
    expect(joinOr(["A"])).toBe("A");
    expect(joinOr(["A", "B"])).toBe("A or B");
    expect(joinOr(["A", "B", "C"])).toBe("A, B or C");
  });
});

describe("setupNeeded: cloud labs", () => {
  it("asks for an account of the lab's cloud first, then for Terraform, then nothing", () => {
    expect(setupNeeded(cloudLab(["aws"]), report(), [])).toBe(translate("labs.readiness.needsCloudAccount", { clouds: "AWS" }));
    expect(setupNeeded(cloudLab(["aws", "gcp"]), report(), [host("azure")])).toBe(
      translate("labs.readiness.needsCloudAccount", { clouds: "AWS or Google Cloud" }),
    );
    expect(setupNeeded(cloudLab(["aws"]), report(), [host("aws")])).toBe(translate("labs.readiness.needsTerraform"));
    expect(setupNeeded(cloudLab(["aws"]), report({ terraform: { installed: true, version: null } }), [host("aws")])).toBeNull();
    expect(needsCloudAccount(cloudLab(["aws"]), [])).toBe(true);
    expect(needsCloudAccount(cloudLab(["aws"]), [host("aws")])).toBe(false);
    expect(needsCloudAccount(vmLab(["aws"]), [])).toBe(false);
  });

  it("a cloud lab on DigitalOcean, Linode or OCI names the cloud it needs an account on", () => {
    // cloudNames knows AWS, Azure and Google Cloud only, so for the other three clouds the
    // launcher supports (CLOUD_META marks them ready) the row reads "Needs an account on ".
    // Suggested fix: add digitalocean, linode and oci to CLOUD_NAMES in lab-runtime.ts, or read
    // the labels from CLOUD_META.
    expect(cloudNames(["digitalocean", "linode", "oci"])).toHaveLength(3);
    expect(setupNeeded(cloudLab(["digitalocean"]), report(), [])).toMatch(/DigitalOcean/);
  });
});

describe("setupNeeded: VM labs", () => {
  it("a lab that lists only servers needs one, and is fine once one of them is saved", () => {
    const r = report({ vmProviders: [provider("virtualbox")] });
    expect(setupNeeded(vmLab(["vmware_esxi"]), r, [])).toBe(translate("labs.readiness.needsServer"));
    expect(setupNeeded(vmLab(["vmware_esxi"]), r, [host("vmware_esxi")])).toBeNull();
    expect(setupNeeded(vmLab(["vmware_esxi"]), r, [host("proxmox")])).toBe(translate("labs.readiness.needsServer"));
  });

  it("names the supported hypervisors this OS knows, Fusion on a Mac", () => {
    const r = report({ os: "macos", vmProviders: [provider("vmware_desktop", false), provider("parallels", false)] });
    expect(setupNeeded(vmLab(["vmware_desktop", "parallels"]), r, [])).toBe(
      translate("labs.readiness.needsHypervisor", { names: "VMware Fusion or Parallels" }),
    );
  });
});

// @vitest-environment jsdom
import { describe, expect, it } from "vitest";
import { choiceNote } from "@/features/machine/setup-steps/vm-steps";
import { freshDebian, freshReport, hypervisorStatus as hv, hypervisors, withNeeds } from "@/test/fixtures";
import { translate } from "@/lib/i18n";
import type { ProviderStatus, SystemReport } from "@/lib/tauri";

/** Each hypervisor card's note on `base`, with `p` its one hypervisor. */
const note = (base: SystemReport, p: ProviderStatus) => {
  const report = withNeeds({ ...base, ...hypervisors([p]) });
  return choiceNote(translate as never, report, report.vmProviders[0]);
};

describe("choiceNote", () => {
  it("says an installed hypervisor runs here, or can't", () => {
    expect(note(freshDebian(), hv({ provider: "virtualbox" }))).toBe(translate("machine.vm.installedHere"));
    const noKvm = hv({ provider: "libvirt", available: false, plugin: "vagrant-libvirt", pluginInstalled: true, reason: { code: "noKvm" } });
    // Nothing left to install for it (Vagrant in), and still it can't run: no /dev/kvm.
    expect(note({ ...freshDebian(), vagrant: { installed: true, version: "2.4.3" } }, noKvm)).toBe(translate("machine.vm.cantRunHere"));
  });

  it("says who gets one that isn't installed: the setup, the player on a distribution without apt, or its vendor", () => {
    const missing = (provider: ProviderStatus["provider"]) => hv({ provider, hypervisor: false, available: false });
    expect(note(freshDebian(), missing("virtualbox"))).toBe(translate("machine.vm.canInstall"));
    expect(note(freshReport("linux"), missing("virtualbox"))).toBe(translate("machine.vm.installYourself"));
    expect(note(freshReport("macos"), missing("parallels"))).toBe(translate("machine.vm.vendorSite"));
  });
});

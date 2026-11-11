import { describe, expect, it } from "vitest";
import { providerInitials } from "@/lib/provider-initials";
import { hypervisorLogo } from "@/features/machine/hypervisor-logo";
import { providerLogo } from "@/features/servers/provider-glyph";
import { LOCAL_PROVIDERS, SERVER_PROVIDERS } from "@/lib/tauri";

describe("providerInitials", () => {
  it("names the providers the launcher has no logo for, the same on every page", () => {
    expect(providerInitials("vmware_esxi")).toBe("ES");
    expect(providerInitials("hyperv")).toBe("HV");
    expect(providerInitials("parallels")).toBe("PD");
    expect(providerInitials("libvirt")).toBe("LV");
    expect(providerInitials("vmware_desktop", "macos")).toBe("VF");
    expect(providerInitials("vmware_desktop", "windows")).toBe("VW");
    expect(providerInitials("vmware_desktop", "linux")).toBe("VW");
  });

  it("covers every server and local hypervisor without a logo, and none with one", () => {
    for (const p of SERVER_PROVIDERS) expect(!!providerInitials(p), p).toBe(!providerLogo(p));
    for (const p of LOCAL_PROVIDERS) expect(!!providerInitials(p, "macos"), p).toBe(!hypervisorLogo(p));
  });
});

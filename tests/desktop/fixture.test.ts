import { describe, expect, it } from "vitest";
import { FIXTURE_MACHINE, libvirtFixtureXml, scanFixture } from "./lib/fixture.mjs";

// The VM vm-scan looks for (lib/fixture.mjs): which one the runner makes on which computer, named
// as the engine names the lab's VMs, and the libvirt domain it defines on Linux.

describe("the scan's fixture VM", () => {
  it("is a UTM VM on macOS, named `<prefix> · <machine>`", () => {
    expect(scanFixture("darwin", "invoice-portal-api-2")).toEqual({ kind: "utm", vm: `invoice-portal-api-2 · ${FIXTURE_MACHINE}` });
  });

  it("is a libvirt domain on Linux, named `<prefix>_<machine>` as the engine names a libvirt VM", () => {
    expect(scanFixture("linux", "invoice-portal-api")).toEqual({ kind: "libvirt", vm: "invoice-portal-api_dc01" });
  });

  it("can't be made elsewhere, Hyper-V's left as a follow-up", () => {
    const made = scanFixture("win32", "invoice-portal-api");
    expect(made).not.toHaveProperty("kind");
    expect("skip" in made && made.skip).toMatch(/^can't run here: .*UTM on macOS and libvirt on Linux only \(Hyper-V's is a later follow-up\)/);
  });
});

describe("the libvirt fixture's domain", () => {
  const xml = libvirtFixtureXml("invoice-portal-api_dc01");

  it("is named for the lab, small, and boots to its firmware: no disk, no network", () => {
    expect(xml).toContain("<name>invoice-portal-api_dc01</name>");
    expect(xml).toContain('<memory unit="MiB">256</memory>');
    expect(xml).toContain("<vcpu>1</vcpu>");
    expect(xml).not.toMatch(/<disk|<interface|<boot /);
  });

  it("runs under KVM on x86_64's q35 by default, under QEMU alone when the computer has no KVM", () => {
    expect(xml).toContain('<domain type="kvm">');
    expect(xml).toContain('<type arch="x86_64" machine="q35">hvm</type>');
    expect(libvirtFixtureXml("x", { kvm: false })).toContain('<domain type="qemu">');
  });

  it("takes the computer's architecture: aarch64 on virt", () => {
    expect(libvirtFixtureXml("x", { arch: "aarch64" })).toContain('<type arch="aarch64" machine="virt">hvm</type>');
  });

  it("escapes a name XML would read otherwise", () => {
    expect(libvirtFixtureXml(`a<b & "c"_dc01`)).toContain("<name>a&lt;b &amp; &quot;c&quot;_dc01</name>");
  });
});

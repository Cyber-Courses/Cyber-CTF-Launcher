// The VM the running-labs scan looks for (specs/vm-scan.mjs, the `fixture-vm` need): made by the
// runner beside the installed fixture lab, named as the engine names that lab's VMs, and dropped
// after the scan. What to make where, and a libvirt domain's definition, kept apart from the
// runner so they can be tested without a hypervisor.

/** The fixture's machine name, after the lab's VM-name prefix. */
export const FIXTURE_MACHINE = "dc01";

/** The fixture VM to make on `platform` for the lab whose VM-name prefix is `prefix` (its isoloom
 *  name, `-<instance>` added when the installed lab holds one): a UTM VM `<prefix> · dc01` on
 *  macOS, a libvirt domain `<prefix>_dc01` (on qemu:///system) on Linux; elsewhere why it can't
 *  be made.
 *  @param {string} platform  process.platform
 *  @param {string} prefix
 *  @returns {{ kind: "utm" | "libvirt", vm: string } | { skip: string }} */
export function scanFixture(platform, prefix) {
  if (platform === "darwin") return { kind: "utm", vm: `${prefix} · ${FIXTURE_MACHINE}` };
  if (platform === "linux") return { kind: "libvirt", vm: `${prefix}_${FIXTURE_MACHINE}` };
  return { skip: "can't run here: the scan's fixture VM is made with UTM on macOS and libvirt on Linux only (Hyper-V's is a later follow-up)" };
}

/** `text` safe inside an XML element or a double-quoted attribute. */
const escapeXml = (text) => text.replace(/[&<>"']/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&apos;" })[c]);

/** A libvirt domain for the fixture, named `name`: 256 MiB, one vCPU, no disk and no network, so
 *  it boots to its firmware and stays running; under KVM when `kvm`, on the computer's own
 *  architecture (`x86_64` on q35, `aarch64` on virt).
 *  @param {string} name
 *  @param {{ arch?: "x86_64" | "aarch64", kvm?: boolean }} [opts]
 *  @returns {string} */
export function libvirtFixtureXml(name, { arch = "x86_64", kvm = true } = {}) {
  const machine = arch === "aarch64" ? "virt" : "q35";
  return [
    `<domain type="${kvm ? "kvm" : "qemu"}">`,
    `  <name>${escapeXml(name)}</name>`,
    `  <memory unit="MiB">256</memory>`,
    `  <vcpu>1</vcpu>`,
    `  <os><type arch="${arch}" machine="${machine}">hvm</type></os>`,
    `  <devices><console type="pty"/></devices>`,
    `</domain>`,
    "",
  ].join("\n");
}

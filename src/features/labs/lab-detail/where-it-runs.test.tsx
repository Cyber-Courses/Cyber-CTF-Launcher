// @vitest-environment jsdom
import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { WhereItRunsPanel } from "@/features/labs/lab-detail/where-it-runs-panel";
import { RunOnPicker } from "@/features/labs/run-on";
import type { LabRuntimeInfo } from "@/features/labs/use-labs";
import { answer } from "@/lib/dev-mock";
import type { ServerList } from "@/lib/tauri";
import { installTauri } from "@/test/tauri";
import { flush } from "@/test/ui";

// Where a lab can run, at rest: the places it can't run are said so, and a VM built for another
// CPU says it will be emulated.

/** The saved servers and accounts of the sample data. */
const HOSTS = (answer("server_list", undefined) as ServerList).hosts;

const x86Vm: LabRuntimeInfo = { runtime: "VM", architectures: ["x86_64"], providers: ["virtualbox"] };

/** The panel of a VM lab built for x86, on an x86 machine with VirtualBox ready, `props` over it. */
const renderPanel = (props: Partial<Parameters<typeof WhereItRunsPanel>[0]> = {}) =>
  render(
    <WhereItRunsPanel
      labId="lab-x"
      runtime={x86Vm}
      hostArch="x86_64"
      readyVms={["virtualbox"]}
      hosts={HOSTS}
      options={null}
      native
      emulates={false}
      isDocker={false}
      meta="VM lab"
      {...props}
    />,
  );

describe("WhereItRunsPanel", () => {
  it("marks the places the lab can't run as not available", async () => {
    installTauri();
    renderPanel();
    await flush();
    expect(screen.getByText("VM on this machine")).toBeTruthy();
    // A VM lab on VirtualBox only: no container, no server, no cloud.
    const unavailable = screen.getAllByTitle("Not available for this lab");
    expect(unavailable.map((row) => row.textContent)).toEqual(["Container on this machinen/a", "Your servern/a", "Your cloud accountn/a"]);
    // A hypervisor is ready, so "VM on this machine" runs now: not marked, no hint.
    expect(screen.getByText("VM on this machine").closest("[title]")).toBeNull();
    expect(document.body.textContent).not.toContain("Emulated");
  });

  it("marks a supported place the launcher isn't set up for as configurable, with what to do", async () => {
    installTauri();
    // No hypervisor ready: the VM this lab supports here is amber and says to install one.
    renderPanel({ readyVms: [] });
    await flush();
    const vmRow = screen.getByText("VM on this machine").closest("[title]");
    expect(vmRow?.getAttribute("title")).toBe("Install a hypervisor");
    expect(vmRow?.textContent).toContain("Install a hypervisor");
  });

  it("says an x86 VM lab runs emulated with QEMU on an Apple Silicon Mac that has it", async () => {
    installTauri();
    renderPanel({ hostArch: "arm64", readyVms: ["qemu"], native: false, emulates: true });
    await flush();
    expect(screen.getByText(/Emulated with QEMU, many times slower than native/)).toBeTruthy();
    // QEMU makes "VM on this machine" a place it can run.
    expect(screen.getByText("VM on this machine").closest("[title]")).toBeNull();
  });

  it("says a foreign-CPU VM lab needs QEMU when none is installed", async () => {
    installTauri();
    renderPanel({ hostArch: "arm64", readyVms: [], native: false });
    await flush();
    expect(screen.getByText(/Built for another CPU: install QEMU/)).toBeTruthy();
    expect(screen.getByText("VM on this machine").closest("[title]")?.getAttribute("title")).toBe("Not available for this lab");
  });

  it("shows the lab's minimum-to-run requirements from its spec", async () => {
    installTauri();
    const vm: LabRuntimeInfo = { runtime: "VM", architectures: ["x86_64"], providers: ["virtualbox"], repository: "CyberCTF/x", commit: "a".repeat(40) };
    renderPanel({ runtime: vm });
    await flush();
    expect(screen.getByText("Needs")).toBeTruthy();
    expect(screen.getByText("2 vCPU · 1 GB RAM · 20 GB disk")).toBeTruthy();
  });
});

describe("RunOnPicker", () => {
  it("keeps a host the lab doesn't support visible, disabled, saying why", async () => {
    installTauri();
    const hosts = HOSTS;
    const aws = hosts.find((h) => h.provider === "aws");
    const esxi = hosts.find((h) => h.provider === "vmware_esxi");
    if (!aws || !esxi) throw new Error("the sample servers lack an AWS account or an ESXi host");
    render(
      <RunOnPicker
        title="GOAD Light"
        hosts={[aws, esxi]}
        hostOk={(h) => h.provider !== "aws"}
        localNote="VirtualBox"
        dockerRunning={null}
        value={{ kind: "local" }}
        disabled={false}
      />,
    );
    await flush();
    const awsRow = screen.getAllByRole("radio").find((r) => r.textContent.includes(aws.name));
    const esxiRow = screen.getAllByRole("radio").find((r) => r.textContent.includes(esxi.name));
    if (!awsRow || !esxiRow) throw new Error("the picker lacks a row per host");
    expect(awsRow.getAttribute("aria-disabled") === "true" || awsRow.hasAttribute("disabled")).toBe(true);
    expect(awsRow.textContent).toContain("Not supported by this lab");
    expect(awsRow.getAttribute("title")).toBe("This lab doesn't support AWS");
    expect(esxiRow.textContent).not.toContain("Not supported by this lab");
  });
});

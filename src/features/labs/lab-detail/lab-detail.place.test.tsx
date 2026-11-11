// @vitest-environment jsdom
import { screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { containerMachine, labMachine, labStatus, testLab } from "@/features/labs/labs-test-kit";
import { renderLabPage } from "@/features/labs/lab-detail/lab-detail-test-kit";
import { installTauri } from "@/test/tauri";
import { flush as flushUi } from "@/test/ui";

// The lab page says where a running lab runs, the same way in its header and its Details.

const flush = () => flushUi(30);
const idle = { busy_labs: () => [], active_operations: () => [] };

describe("LabDetail, where it runs", () => {
  it("names the engine a container lab runs on here, recorded at launch", async () => {
    installTauri(idle);
    renderLabPage(testLab("lab-web"), {
      status: labStatus({ running: true, place: "container", provider: "orbstack", machines: [containerMachine("web")] }),
      dockerEngine: "docker-desktop",
    });
    await flush();
    expect(document.body.textContent).toContain("on this machine · OrbStack");
    expect(screen.getByText("Runs on").parentElement?.textContent).toContain("this machine · OrbStack");
  });

  it("names the machine's current engine when the lab wasn't pinned to one", async () => {
    installTauri(idle);
    renderLabPage(testLab("lab-web"), {
      status: labStatus({ running: true, place: "container", provider: "docker", machines: [containerMachine("web")] }),
      dockerEngine: "colima",
    });
    await flush();
    expect(document.body.textContent).toContain("on this machine · Colima");
  });

  it("names the hypervisor a VM lab runs on here", async () => {
    installTauri(idle);
    renderLabPage(testLab("lab-ad", { runtime: { runtime: "VM", architectures: ["x86_64"], providers: ["qemu", "virtualbox"] } }), {
      status: labStatus({ running: true, place: "local_vm", provider: "qemu", machines: [labMachine("dc01")] }),
      readyVms: ["qemu"],
    });
    await flush();
    expect(document.body.textContent).toContain("on this machine · QEMU");
    expect(screen.getByText("Runs on").parentElement?.textContent).toContain("this machine · QEMU");
  });
});

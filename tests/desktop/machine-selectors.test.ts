// @vitest-environment jsdom
import { createElement } from "react";
import { render } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { expect, it, vi } from "vitest";
import { MachineScreen } from "@/features/machine/machine-screen";
import { hypervisorStatus as hv, hypervisors, readyReport } from "@/test/fixtures";
import { installTauri, rejectWith, stream } from "@/test/tauri";
import { flush } from "@/test/ui";
import { providerTestButton, selfTestOutcome } from "./lib/support.mjs";

// The machine spec's selectors (lib/support.mjs) run on the Machine page itself, rendered as the
// app renders it: a WebdriverIO browser's `$` stood in for by the document's own XPath.

/** What the specs' selectors take for WebdriverIO's browser: `$` by XPath, its first match. */
const browser = {
  $: (xpath: string) => {
    const el = document.evaluate(xpath, document, null, XPathResult.FIRST_ORDERED_NODE_TYPE, null).singleNodeValue as HTMLElement | null;
    return {
      el,
      isExisting: () => Promise.resolve(el !== null),
      getAttribute: (name: string) => Promise.resolve(el?.getAttribute(name) ?? null),
      getText: () => Promise.resolve(el?.textContent ?? ""),
    };
  },
};

const ev = (step: string, state: string, detail: string | null = null) => ({ step, label: step, state, detail });

it("the VM lab test is the named hypervisor's row's and its end is read from the panel", async () => {
  // jarvis, debian13-quick, e2e:vm (CYBERCTF_TEST_VM_PROVIDER=libvirt): VirtualBox the default
  // row, libvirt and QEMU ready beside it, the container test run first from the engine's row.
  let finishVm: () => void = () => {};
  const calls = installTauri({
    machine_selftest: (a) => {
      if (a?.kind === "docker") {
        stream(a, ev("engine", "fail", "Cannot connect to the Docker daemon"));
        rejectWith("engine failed");
      }
      stream(a, ev("vagrant", "ok"), ev("provider", "running"));
      return new Promise<null>((resolve) => {
        finishVm = () => {
          stream(a, ev("provider", "ok"), ev("boot", "ok"), ev("cleanup", "ok"));
          resolve(null);
        };
      });
    },
  });
  const report = {
    ...readyReport(),
    os: "linux",
    ...hypervisors([hv({ provider: "virtualbox" }), hv({ provider: "libvirt", plugin: "vagrant-libvirt" }), hv({ provider: "qemu", plugin: "vagrant-qemu" })]),
  };
  render(createElement("main", null, createElement(MachineScreen, { report, onRefresh: vi.fn(), onNavigate: vi.fn() })));
  await flush(50);
  const user = userEvent.setup();

  // The container test, from the first Test on the page (the engine labs use); it fails at once.
  await user.click(browser.$('(//main//button[normalize-space()="Test"])[1]').el as HTMLElement);
  await flush(50);
  const container = await selfTestOutcome(browser, "Container lab test");
  expect(container?.ok).toBe(false);
  expect(container?.text).toContain("Cannot connect to the Docker daemon");

  // libvirt's own Test, though "libvirt" is written elsewhere on the page (Machine details).
  const test = providerTestButton(browser, "libvirt") as ReturnType<typeof browser.$>;
  expect(test.el?.closest('[role="listitem"]')?.querySelector('[data-slot="name"]')?.textContent).toBe("libvirt");
  await user.click(test.el as HTMLElement);
  await flush(50);
  expect(calls.filter((c) => c.cmd === "machine_selftest").map((c) => c.args?.provider)).toEqual([null, "libvirt"]);
  // Running: no outcome yet. Ended: read, though the status shows the run's time beside it.
  expect(await selfTestOutcome(browser, "VM lab test")).toBeNull();
  finishVm();
  await flush(50);
  const vm = await selfTestOutcome(browser, "VM lab test");
  expect(vm?.ok).toBe(true);
  expect(vm?.text).toMatch(/Passed\d/);
});

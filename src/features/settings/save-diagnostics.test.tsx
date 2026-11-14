// @vitest-environment jsdom
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it } from "vitest";
import { AboutSection } from "@/features/settings/about-section";
import { SUPPORT_EMAIL, bundleFileName, bundleSettings } from "@/features/settings/diagnostics";
import { Toaster } from "@/components/ui/toaster";
import { toastsShown } from "@/components/test-support";
import { setAttackImage, setVmProvider } from "@/lib/settings";
import type { AgentInfo } from "@/lib/tauri";
import { commands, installTauri, rejects, type Call } from "@/test/tauri";
import { button, flush } from "@/test/ui";

// Settings > About > Save diagnostics: the bundle saved where the player picks, with the
// window's settings, and a line saying to attach it to the report.

const AGENT: AgentInfo = { installId: "c0ffee", name: "mbp", arch: "arm64", capabilities: [], state: "online" };
const PICKED = "/home/player/Desktop/cyberctf-diagnostics.tar.gz";

/** The About section, signed in, with the Toaster so what it tells shows. */
function renderAbout() {
  render(
    <>
      <AboutSection version="1.4.0" report={null} agent={AGENT} updates="inApp" />
      <Toaster />
    </>,
  );
}

/** The arguments `save_diagnostics` was invoked with, if it was. */
const saved = (calls: Call[]) => calls.find((c) => c.cmd === "save_diagnostics")?.args;

beforeEach(() => {
  toastsShown();
});

describe("Save diagnostics", () => {
  it("sits beside Copy and says to attach the file to the report", () => {
    installTauri();
    renderAbout();
    expect(button(/^\s*copy$/i)).toBeTruthy();
    expect(button(/save diagnostics/i)).toBeTruthy();
    expect(document.body.textContent).toContain("attach it to your bug report");
  });

  it("saves the bundle where the player picked, with the window's settings, then says so", async () => {
    setAttackImage("kalilinux/kali-rolling");
    setVmProvider("virtualbox");
    const calls = installTauri({ "plugin:dialog|save": () => PICKED, save_diagnostics: () => null });
    const user = userEvent.setup();
    renderAbout();
    await user.click(button(/save diagnostics/i));
    await flush();
    const dialog = calls.find((c) => c.cmd === "plugin:dialog|save")?.args as { options?: { defaultPath?: string } } | undefined;
    expect(dialog?.options?.defaultPath).toMatch(/^cyberctf-diagnostics-\d{8}-\d{4}\.tar\.gz$/);
    expect(saved(calls)?.path).toBe(PICKED);
    expect(saved(calls)?.settings).toMatchObject({ attackImage: "kalilinux/kali-rolling", vmProvider: "virtualbox", installId: "c0ffee" });
    expect(screen.getByRole("status").textContent).toBe(`Saved to ${PICKED}. Attach this file to your bug report.`);
    expect(commands(calls)).not.toContain("plugin:opener|open_url");
  });

  it("sends to support: saves the bundle, then opens a mail naming the version and where the file is", async () => {
    const calls = installTauri({ "plugin:dialog|save": () => PICKED, save_diagnostics: () => null, "plugin:opener|open_url": () => null });
    const user = userEvent.setup();
    renderAbout();
    await user.click(button(/send to support/i));
    await flush();
    expect(commands(calls).indexOf("save_diagnostics")).toBeLessThan(commands(calls).indexOf("plugin:opener|open_url"));
    const url = new URL(String(calls.find((c) => c.cmd === "plugin:opener|open_url")?.args?.url));
    expect(`${url.protocol}${url.pathname}`).toBe(`mailto:${SUPPORT_EMAIL}`);
    expect(url.searchParams.get("subject")).toBe("Cyber CTF 1.4.0: bug report");
    expect(url.searchParams.get("body")).toContain(`The diagnostics bundle is attached (saved at ${PICKED}).`);
  });

  it("opens no mail when the bundle couldn't be saved", async () => {
    const calls = installTauri({ "plugin:dialog|save": () => PICKED, save_diagnostics: rejects({ code: "other", message: "the disk is full" }) });
    const user = userEvent.setup();
    renderAbout();
    await user.click(button(/send to support/i));
    await flush();
    expect(commands(calls)).not.toContain("plugin:opener|open_url");
    expect(
      screen
        .queryAllByRole("alert")
        .map((a) => a.textContent)
        .join("\n"),
    ).toContain("Couldn't prepare the mail to support");
  });

  it("is reached from the keyboard, each control in a labelled group", async () => {
    installTauri();
    const user = userEvent.setup();
    renderAbout();
    const group = screen.getByRole("group", { name: "Diagnostics" });
    const names = [...group.querySelectorAll("button")].map((b) => b.textContent.trim());
    expect(names).toEqual(["Copy", "Save diagnostics", "Send to support"]);
    button(/^\s*copy$/i).focus();
    await user.tab();
    expect(document.activeElement).toBe(button(/save diagnostics/i));
    await user.tab();
    expect(document.activeElement).toBe(button(/send to support/i));
  });

  it("saves nothing when the player closes the dialog", async () => {
    const calls = installTauri({ "plugin:dialog|save": () => null });
    const user = userEvent.setup();
    renderAbout();
    await user.click(button(/save diagnostics/i));
    await flush();
    expect(commands(calls)).not.toContain("save_diagnostics");
    expect(screen.queryByRole("alert")).toBeNull();
  });

  it("says why when the bundle can't be saved, and lets the player try again", async () => {
    installTauri({ "plugin:dialog|save": () => PICKED, save_diagnostics: rejects({ code: "other", message: "the disk is full" }) });
    const user = userEvent.setup();
    renderAbout();
    await user.click(button(/save diagnostics/i));
    await flush();
    const alerts = screen
      .queryAllByRole("alert")
      .map((a) => a.textContent)
      .join("\n");
    expect(alerts).toContain("Couldn't save the diagnostics");
    expect(alerts).toContain("the disk is full");
    expect(button(/save diagnostics/i).hasAttribute("disabled")).toBe(false);
  });
});

describe("the bundle's settings and name", () => {
  it("carries the launch and attack box settings, and the install id only when signed in", () => {
    setVmProvider(null);
    expect(bundleSettings(null)).not.toHaveProperty("installId");
    expect(bundleSettings(null).vmProvider).toBe("automatic");
    expect(bundleSettings(AGENT).installId).toBe("c0ffee");
  });

  it("names the file after the local date and time", () => {
    expect(bundleFileName(new Date(2026, 10, 14, 6, 5))).toBe("cyberctf-diagnostics-20261114-0605.tar.gz");
  });
});

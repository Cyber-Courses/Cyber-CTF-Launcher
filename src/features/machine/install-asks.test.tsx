// @vitest-environment jsdom
import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { DownloadByHand, downloadOf } from "@/features/machine/download-by-hand";
import { InstallConsent, InstallHint, agreementsOf, asksOf } from "@/features/machine/install-asks";
import type { Interaction, SystemReport } from "@/lib/tauri";
import { SETUP_SHAPES } from "@/lib/tauri/generated/SetupShapes";
import { PlanStep, setupShape } from "@/features/machine/machine-test-kit";
import { commands, held, installTauri, progressLines, settled, stream } from "@/test/tauri";
import { button } from "@/test/ui";

// What an install asks of the player (the core's `install::Interaction`s, which the setup options'
// install actions carry): said before it starts, in a dialog that only informs (a vendor's agreement
// is the player's to accept in the vendor's own window: nothing here accepts it), and again while
// its step runs, above its log.

const DOCKER_URL = "https://www.docker.com/legal/docker-subscription-service-agreement/";
const DOCKER_AGREEMENT: Interaction = { kind: "vendorAgreement", app: "Docker Desktop", name: "Docker Subscription Service Agreement", url: DOCKER_URL };
const MAC_PASSWORD: Interaction = { kind: "adminPassword", via: "mac" };

describe("the consent before an install", () => {
  it("says each thing the install asks, the agreement Docker Desktop will show to accept there, with nothing to tick", async () => {
    const onInstall = vi.fn();
    const user = userEvent.setup();
    const asks: Interaction[] = [
      MAC_PASSWORD,
      DOCKER_AGREEMENT,
      { kind: "allowSystemExtension", vendor: "Oracle America, Inc." },
      { kind: "restart" },
      { kind: "longBuild", minutes: 15 },
    ];
    render(<InstallConsent pending={{ name: "Docker Desktop", asks }} onInstall={onInstall} onCancel={vi.fn()} />);
    const dialog = screen.getByRole("dialog", { name: "Before Docker Desktop installs" });
    const said = within(dialog)
      .getAllByRole("listitem")
      .map((li) => li.textContent);
    expect(said).toEqual([
      "macOS will ask for your password, in a window of its own.",
      "Docker Desktop will show the Docker Subscription Service Agreement when it first starts. Accept it there to use it: Cyber CTF never accepts it for you.",
      "You'll need to allow the system extension from Oracle America, Inc. in System Settings › Privacy & Security.",
      "You'll need to restart your computer afterwards.",
      "This builds from source and can take about 15 minutes.",
    ]);
    // Regression: an "I accept" tick made the install accept Docker's agreement for the player.
    expect(within(dialog).queryAllByRole("checkbox")).toEqual([]);
    const calls = installTauri();
    await user.click(within(dialog).getByRole("button", { name: "Read the Docker Subscription Service Agreement" }));
    expect(calls.find((c) => c.cmd === "plugin:opener|open_url")?.args?.url).toBe(DOCKER_URL);
    const install = within(dialog).getByRole("button", { name: "Install" });
    expect(install.hasAttribute("disabled")).toBe(false);
    await user.click(install);
    expect(onInstall).toHaveBeenCalledOnce();
  });

  it("finds what an install asks in the setup options the core worked out", () => {
    const windows = setupShape("bareWindows");
    expect(asksOf(windows, "docker")).toEqual([{ kind: "adminPassword", via: "uac" }, DOCKER_AGREEMENT]);
    expect(asksOf(windows, "wsl")).toEqual([{ kind: "adminPassword", via: "uac" }, { kind: "restart" }]);
    expect(agreementsOf(asksOf(setupShape("macDockerWithoutVagrant"), "vagrant"))).toEqual([]);
    expect(agreementsOf(asksOf(setupShape("macDockerWithoutVagrant"), "docker"))).toEqual([]);
    expect(asksOf(setupShape("macDockerWithoutVagrant"), "vagrant")).toEqual([MAC_PASSWORD]);
    expect(asksOf(null, "docker")).toEqual([]);
  });
});

describe("the live hint while an install runs", () => {
  it("says what the step running asks right now", () => {
    render(<InstallHint asks={[MAC_PASSWORD, { kind: "relogin" }]} />);
    const hint = screen.getByRole("status");
    expect(hint.textContent).toContain("macOS is asking for your password in a window that just opened.");
    expect(hint.textContent).toContain("When it's done, sign out and back in.");
  });

  it("is nothing when the step asks nothing", () => {
    const { container } = render(<InstallHint asks={[]} />);
    expect(container.textContent).toBe("");
  });
});

describe("an install from the setup window", () => {
  it("says first what it asks, accepts nothing, shows the step's hint while it runs, then drops it", async () => {
    let finish: () => void = () => {};
    const calls = installTauri({
      install_dependency: held(
        (a) =>
          new Promise<null>((done) => {
            stream(a, { kind: "asks", interactions: [{ kind: "adminPassword", via: "uac" }, DOCKER_AGREEMENT] }, ...progressLines("Starting the installer"));
            finish = () => {
              done(null);
            };
          }),
      ),
    });
    const user = userEvent.setup();
    render(<PlanStep step="docker" report={setupShape("bareWindows")} />);
    await user.click(button(/install docker desktop/i));
    const dialog = await screen.findByRole("dialog", { name: "Before Docker Desktop installs" });
    expect(dialog.textContent).toContain("Windows will ask you to allow the changes (User Account Control).");
    expect(commands(calls)).not.toContain("install_dependency");
    await user.click(within(dialog).getByRole("button", { name: "Install" }));
    const args = calls.find((c) => c.cmd === "install_dependency")?.args;
    expect(args).toMatchObject({ dependency: "docker" });
    expect(args).not.toHaveProperty("licences");
    const hint = await screen.findByTestId("install-hint");
    expect(hint.textContent).toContain("Windows is asking you to allow the changes in a window that just opened.");
    expect(hint.textContent).toContain("When Docker Desktop first starts, it shows the Docker Subscription Service Agreement: accept it in its window.");
    expect(document.body.textContent).toContain("Starting the installer");
    finish();
    await settled();
    expect(screen.queryByTestId("install-hint")).toBeNull();
  });

  it("installs nothing when the player cancels", async () => {
    const calls = installTauri();
    const user = userEvent.setup();
    render(<PlanStep step="docker" report={setupShape("bareWindows")} />);
    await user.click(button(/install docker desktop/i));
    const dialog = await screen.findByRole("dialog");
    await user.click(within(dialog).getByRole("button", { name: "Cancel" }));
    expect(screen.queryByRole("dialog")).toBeNull();
    expect(commands(calls)).not.toContain("install_dependency");
  });
});

describe("a tool only its vendor offers", () => {
  it("is a download by hand from its vendor's page on each OS, never left out", () => {
    const vmware = (name: keyof typeof SETUP_SHAPES) => downloadOf(asksOfVmware(setupShape(name)));
    expect(vmware("bareWindows")).toMatchObject({ vendor: "Broadcom", url: expect.stringContaining("VMware%20Workstation%20Pro") as string });
    expect(vmware("macDockerWithoutVagrant")).toMatchObject({ vendor: "Broadcom", url: expect.stringContaining("VMware%20Fusion") as string });
  });

  it("links to the page, checks again, and says when the tool still isn't there", async () => {
    const calls = installTauri();
    const user = userEvent.setup();
    const onCheck = vi.fn();
    const page = { kind: "downloadByHand", vendor: "Broadcom", url: "https://support.broadcom.com/group/ecx/downloads" } as const;
    const report = setupShape("bareWindows");
    const { rerender } = render(<DownloadByHand name="VMware Workstation" page={page} report={report} onCheck={onCheck} />);
    await user.click(screen.getByRole("button", { name: "Open Broadcom's download page" }));
    expect(calls.find((c) => c.cmd === "plugin:opener|open_url")?.args?.url).toBe(page.url);
    await user.click(screen.getByRole("button", { name: "Check again" }));
    expect(onCheck).toHaveBeenCalledOnce();
    expect(screen.getByRole("status").textContent).toBe("Checking this machine…");
    expect(screen.getByRole("button", { name: "Check again" }).hasAttribute("disabled")).toBe(true);
    // The check answered and the tool is still missing (the row is still shown).
    rerender(<DownloadByHand name="VMware Workstation" page={page} report={{ ...report }} onCheck={onCheck} />);
    expect(screen.getByRole("status").textContent).toBe("VMware Workstation isn't installed yet. Finish its install, then check again.");
  });
});

/** What getting VMware's desktop hypervisor asks on the machine `r` reports. */
function asksOfVmware(r: SystemReport): Interaction[] {
  const get = r.setup.hypervisors.find((h) => h.provider === "vmware_desktop")?.get[0];
  return get && "interactions" in get ? get.interactions : [];
}

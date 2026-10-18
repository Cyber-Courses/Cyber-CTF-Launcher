// @vitest-environment jsdom
import { act, fireEvent, render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";
import { AppearanceSection } from "@/features/settings/appearance-section";
import { InstalledToolsPanel } from "@/features/settings/installed-tools-panel";
import { SettingsScreen } from "@/features/settings/settings-screen";
import { SharedFolderControl } from "@/features/settings/shared-folder";
import { must } from "@/components/test-support";
import { openCustomImage, renderInstalledTools, toggleFailingSharedFolder } from "@/features/settings/settings-test-kit";
import { getLocalePreference, setLocalePreference } from "@/lib/i18n";
import { DEFAULT_ATTACK_IMAGE, getAttackImage } from "@/lib/settings";
import { installTauri, stream, rejectWith } from "@/test/tauri";
import { button, flush } from "@/test/ui";

// Settings driven the way a player does: with the mouse and the keyboard, across the rail's
// sections. Complements settings-rows.test.tsx, which covers each row on its own.

const text = () => document.body.textContent;
const signedIn = { loggedIn: true, name: "Ada", email: "ada@example.com" };

afterEach(() => {
  act(() => {
    setLocalePreference("system");
  });
  document.documentElement.classList.remove("light", "black");
  vi.restoreAllMocks();
});

async function renderSettings() {
  render(<SettingsScreen auth={signedIn} onAuthChange={vi.fn()} onNavigate={vi.fn()} />);
  await flush();
  const user = userEvent.setup();
  const open = async (name: RegExp) => {
    await user.click(within(screen.getByRole("navigation")).getByRole("button", { name }));
    await flush();
  };
  return { user, open };
}

describe("Settings: appearance", () => {
  it("applies the theme to the page at once and remembers it for the next launch", async () => {
    installTauri();
    const user = userEvent.setup();
    render(<AppearanceSection />);
    await user.click(screen.getByRole("radio", { name: "Light" }));
    expect(document.documentElement.classList.contains("light")).toBe(true);
    expect(localStorage.getItem("cyberctf.appearance")).toBe("light");
    await user.click(screen.getByRole("radio", { name: "Black" }));
    expect(document.documentElement.classList.contains("black")).toBe(true);
    expect(document.documentElement.classList.contains("light")).toBe(false);
    // Dark is the base look: no class at all.
    await user.click(screen.getByRole("radio", { name: "Dark" }));
    expect(document.documentElement.className).not.toMatch(/light|black/);
  });

  it("follows a theme picked in another window", async () => {
    installTauri();
    render(<AppearanceSection />);
    await flush();
    act(() => {
      window.dispatchEvent(new StorageEvent("storage", { key: "cyberctf.appearance", newValue: "light" }));
    });
    expect(screen.getByRole("radio", { name: "Light" }).getAttribute("aria-checked")).toBe("true");
    expect(document.documentElement.classList.contains("light")).toBe(true);
  });

  it("switches the theme from the keyboard", async () => {
    installTauri();
    const user = userEvent.setup();
    render(<AppearanceSection />);
    screen.getByRole("radio", { name: "Black" }).focus();
    await user.keyboard("{Enter}");
    expect(screen.getByRole("radio", { name: "Black" }).getAttribute("aria-checked")).toBe("true");
  });

  it("translates the whole settings window, rail included, when the language changes", async () => {
    installTauri();
    const { user, open } = await renderSettings();
    await open(/appearance/i);
    await user.selectOptions(screen.getByRole("combobox", { name: /language/i }), "fr");
    expect(getLocalePreference()).toBe("fr");
    const rail = within(screen.getByRole("navigation"));
    expect(rail.getByRole("button", { name: /compte/i })).toBeTruthy();
    expect(rail.getByRole("button", { name: /apparence/i }).getAttribute("aria-current")).toBe("page");
    // The section stays open on the same tab, now in French.
    expect(screen.getByRole("combobox").getAttribute("aria-label")).not.toBe("Language");
  });
});

describe("Settings: shared folder", () => {
  it("clears an earlier error once a save works", async () => {
    const { save, toggle } = await toggleFailingSharedFolder();
    expect(text()).toContain("permission denied");
    save.fail = false;
    await toggle();
    expect(text()).not.toContain("permission denied");
    expect(screen.getByRole("switch").getAttribute("aria-checked")).toBe("true");
  });

  it("names its switch after the setting, so it can be found without seeing it", async () => {
    installTauri();
    render(<SharedFolderControl />);
    await flush();
    expect(screen.getByRole("switch", { name: /share/i })).toBeTruthy();
  });

  // Bug: shared-folder.tsx:34 awaits the folder picker with no catch. When the dialog can't open
  // (a denied permission, a sandbox), `void choose()` drops the rejection: nothing is shown and
  // the press seems ignored. Suggested fix: wrap the `open()` call in try/catch and set `error`.
  it("says why the folder picker couldn't open", async () => {
    installTauri({
      shared_folder_get: () => ({ enabled: true, path: "/Users/player/CyberCTF", mountPoint: "/shared" }),
      "plugin:dialog|open": () => {
        rejectWith("dialog not allowed");
      },
    });
    const user = userEvent.setup();
    render(<SharedFolderControl />);
    await flush();
    await user.click(button(/change/i));
    await flush();
    expect(text()).toContain("dialog not allowed");
  });
});

describe("Settings: attack box image", () => {
  it("can't save a custom image made only of spaces, and saves a real one with Enter", async () => {
    const { user, onSaved, field, save } = await openCustomImage();
    const input = field();
    // The field takes the focus as it opens, so the player can type at once.
    expect(document.activeElement).toBe(input);
    await user.type(input, "   ");
    expect(save().hasAttribute("disabled")).toBe(true);
    await user.type(input, "{Enter}");
    expect(getAttackImage()).toBe(DEFAULT_ATTACK_IMAGE);
    await user.clear(input);
    await user.type(input, "ghcr.io/me/box:2{Enter}");
    expect(getAttackImage()).toBe("ghcr.io/me/box:2");
    expect(onSaved).toHaveBeenCalledOnce();
  });

  it("goes back to a preset after a custom image, selecting only that preset", async () => {
    const { user, field } = await openCustomImage();
    await user.type(field(), "ghcr.io/me/box:2{Enter}");
    const kali = must(
      screen.getAllByRole("radio").find((r) => r.textContent.includes("kalilinux/kali-rolling")),
      "the Kali preset",
    );
    await user.click(kali);
    expect(getAttackImage()).toBe("kalilinux/kali-rolling");
    expect(kali.getAttribute("aria-checked")).toBe("true");
    expect(screen.getByRole("radio", { name: /custom image/i }).getAttribute("aria-checked")).toBe("false");
  });

  // Bug: attack-box-rows.tsx:99 saves any custom text as the image. The core only accepts a
  // plain `registry/name:tag@digest` (exegol.rs valid_image: ASCII letters, digits and . _ - / :
  // @, no leading "-"), so "kali linux" or "-v" is saved here without a word and fails only
  // later, when the attack box starts beside a lab. Suggested fix: check the same rule before
  // enabling Save, and say what is wrong under the field.
  it("refuses a custom image name the core can't run", async () => {
    const { user, field, save } = await openCustomImage();
    await user.type(field(), "kali linux:latest");
    expect(save().hasAttribute("disabled")).toBe(true);
    await user.type(field(), "{Enter}");
    expect(getAttackImage()).toBe(DEFAULT_ATTACK_IMAGE);
  });
});

describe("Settings: removing tools Cyber CTF installed", () => {
  const tools = () => [
    { dependency: "vagrant", at: Math.floor(Date.now() / 1000) - 3600 },
    { dependency: "terraform", at: Math.floor(Date.now() / 1000) - 60 },
  ];

  it("cancels the removal on Escape, with nothing uninstalled", async () => {
    const { calls, user, askToRemove } = await renderInstalledTools({ installed_tools: tools });
    await askToRemove();
    expect(screen.getByRole("alertdialog")).toBeTruthy();
    await user.keyboard("{Escape}");
    expect(screen.queryByRole("alertdialog")).toBeNull();
    expect(calls.map((c) => c.cmd)).not.toContain("uninstall_dependency");
  });

  it("puts the focus on Cancel when the confirmation opens, so Enter keeps the tool", async () => {
    const { calls, user, askToRemove } = await renderInstalledTools({ installed_tools: tools });
    await askToRemove();
    const dialog = screen.getByRole("alertdialog");
    // jsdom has no layout (offsetParent is always null), so the trap can only fall back to the
    // dialog itself; pressing Enter must still never confirm.
    expect(dialog.contains(document.activeElement)).toBe(true);
    await user.keyboard("{Enter}");
    expect(calls.map((c) => c.cmd)).not.toContain("uninstall_dependency");
  });

  it("streams the removal log, keeps every Remove off while it runs, and frees them after", async () => {
    let finish: () => void = () => {};
    let list = tools();
    const calls = installTauri({
      installed_tools: () => list,
      uninstall_dependency: (a) =>
        new Promise<null>((r) => {
          stream(a, "==> Uninstalling vagrant");
          finish = () => {
            list = list.filter((t) => t.dependency !== a?.dependency);
            r(null);
          };
        }),
    });
    const user = userEvent.setup();
    render(<InstalledToolsPanel />);
    await flush();
    await user.click(screen.getAllByRole("button", { name: /remove/i })[0]);
    await user.click(within(screen.getByRole("alertdialog")).getByRole("button", { name: /^remove$/i }));
    await flush();
    expect(text()).toContain("==> Uninstalling vagrant");
    expect(text()).toContain("Removing…");
    // The Remove button of each tool's row (the log's own header button is not one).
    const rowButtons = () => Array.from(document.querySelectorAll<HTMLButtonElement>("li button"));
    expect(rowButtons().every((b) => b.hasAttribute("disabled"))).toBe(true);
    // A second press on another tool during the removal can't start a second uninstall.
    fireEvent.click(rowButtons()[1]);
    expect(screen.queryByRole("alertdialog")).toBeNull();
    finish();
    await flush();
    expect(calls.filter((c) => c.cmd === "uninstall_dependency")).toHaveLength(1);
    // Vagrant is gone from the list (the log above still names it).
    expect(Array.from(document.querySelectorAll("li")).map((li) => li.textContent)).not.toContainEqual(expect.stringContaining("Vagrant"));
    expect(rowButtons().every((b) => !b.hasAttribute("disabled"))).toBe(true);
  });

  it("shows the tools in the Maintenance section of the settings window", async () => {
    installTauri({ installed_tools: tools });
    const { open } = await renderSettings();
    await open(/maintenance/i);
    expect(text()).toContain("Terraform");
    expect(text()).toContain("Vagrant");
  });
});

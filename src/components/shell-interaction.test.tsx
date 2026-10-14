// @vitest-environment jsdom
import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { emit } from "@tauri-apps/api/event";
import { FlaskConical, Search } from "lucide-react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { AppShell } from "@/components/app-shell";
import { CommandPalette, type Command } from "@/components/command-palette";
import { UpdateBanner } from "@/components/update-banner";
import { ConfirmDialog } from "@/components/ui/confirm-dialog";
import { LaunchConfirm } from "@/features/app/launch-confirm";
import { QuitGuard } from "@/features/app/quit-guard";
import { commands, installTauri } from "@/test/tauri";
import { flush } from "@/test/ui";

// The updater plugin: `check()` answers what `updater.pending` holds.
const updater = vi.hoisted(() => ({
  pending: null as null | { version: string; downloadAndInstall: () => Promise<void> },
}));
vi.mock("@tauri-apps/plugin-updater", () => ({ check: async () => updater.pending }));

// The deep-link plugin: the link the app was opened with, and the handler for links opened later.
const links = vi.hoisted(() => ({
  launch: null as string[] | null,
  onOpen: null as null | ((urls: string[]) => void),
}));
vi.mock("@tauri-apps/plugin-deep-link", () => ({
  getCurrent: async () => links.launch,
  onOpenUrl: async (fn: (urls: string[]) => void) => {
    links.onOpen = fn;
    return () => {
      if (links.onOpen === fn) links.onOpen = null;
    };
  },
}));

const onMac = () => vi.spyOn(navigator, "userAgent", "get").mockReturnValue("Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7)");
const onLinux = () => vi.spyOn(navigator, "userAgent", "get").mockReturnValue("Mozilla/5.0 (X11; Linux x86_64)");

// jsdom lays nothing out, so `offsetParent` is always null and the focus trap sees no focusable
// element: give every element a layout parent, as in the real webview.
let offsetParent: PropertyDescriptor | undefined;
beforeEach(() => {
  updater.pending = null;
  links.launch = null;
  offsetParent = Object.getOwnPropertyDescriptor(HTMLElement.prototype, "offsetParent");
  Object.defineProperty(HTMLElement.prototype, "offsetParent", { configurable: true, get: () => document.body });
});
afterEach(() => {
  if (offsetParent) Object.defineProperty(HTMLElement.prototype, "offsetParent", offsetParent);
  vi.restoreAllMocks();
});

/** The main window, past the onboarding, once its first reads have settled. */
async function shell() {
  localStorage.setItem("cyberctf.onboarded", "1");
  render(<AppShell />);
  await screen.findByRole("navigation");
  await flush(60);
}

const current = () => screen.getByRole("button", { current: "page" }).textContent ?? "";
const crumbs = () => document.querySelector("[data-tauri-drag-region].h-12")?.textContent ?? "";

describe("deep links into the main window", () => {
  // First in the file: the link the app was opened with is read once per session (module state).
  it("opens the lab the app was launched with", async () => {
    links.launch = ["https://example.com/", "cyberctf://labs/invoice-portal-api"];
    installTauri();
    await shell();
    await flush(80);
    expect(current()).toContain("Labs");
    expect(crumbs()).toContain("Invoice portal API");
  });

  it("opens a lab linked while the app runs, whatever screen is open", async () => {
    installTauri();
    const user = userEvent.setup();
    await shell();
    await user.click(screen.getByRole("button", { name: /^cloud/i }));
    expect(current()).toContain("Cloud");
    await act(async () => links.onOpen?.(["cyberctf://labs/sqli-blind-orders"]));
    await flush(80);
    expect(current()).toContain("Labs");
    expect(crumbs()).toContain("Blind orders");
  });

  it("ignores links that aren't lab links, and never starts anything", async () => {
    const calls = installTauri();
    await shell();
    await act(async () => links.onOpen?.(["cyberctf://settings/x", "cyberctf://labs/Bad_Slug", "https://cyberctf.com/labs/goad-light"]));
    await flush(50);
    expect(current()).toContain("Overview");
    // A web page can show a lab, never start one.
    expect(commands(calls)).not.toContain("lab_launch");
  });
});

describe("CommandPalette keyboard", () => {
  const cmds = (run = vi.fn()): Command[] => [
    { id: "find", label: "Find a lab", hint: "/", icon: Search, keywords: "labs search", group: "Actions", run: () => run("find") },
    { id: "go-home", label: "Go to Overview", icon: Search, group: "Screens", run: () => run("home") },
    { id: "go-cloud", label: "Go to Cloud", icon: Search, group: "Screens", run: () => run("cloud") },
    { id: "lab-a", label: "Blind orders", hint: "Web", icon: FlaskConical, keywords: "lab web", group: "Labs", run: () => run("lab-a") },
    { id: "lab-b", label: "GOAD Light", hint: "Active Directory", icon: FlaskConical, keywords: "lab ad", group: "Labs", run: () => run("lab-b") },
  ];
  const active = () => document.querySelector("[data-active=true]")?.textContent ?? "";
  const field = () => screen.getByRole("dialog").querySelector("input")!;

  it("focuses its search field and selects the first command on open", async () => {
    render(<CommandPalette open onClose={vi.fn()} commands={cmds()} />);
    await waitFor(() => expect(document.activeElement).toBe(field()));
    expect(active()).toContain("Find a lab");
  });

  it("wraps from the first command to the last with ArrowUp", async () => {
    const user = userEvent.setup();
    render(<CommandPalette open onClose={vi.fn()} commands={cmds()} />);
    await waitFor(() => expect(document.activeElement).toBe(field()));
    await user.keyboard("{ArrowUp}");
    expect(active()).toContain("GOAD Light");
    await user.keyboard("{ArrowDown}");
    expect(active()).toContain("Find a lab");
  });

  it("keeps the selection in range when typing narrows the list, so Enter runs the one left", async () => {
    const run = vi.fn();
    const onClose = vi.fn();
    const user = userEvent.setup();
    render(<CommandPalette open onClose={onClose} commands={cmds(run)} />);
    await waitFor(() => expect(document.activeElement).toBe(field()));
    // Down to the last row, then a query only one command matches.
    for (let i = 0; i < 4; i++) await user.keyboard("{ArrowDown}");
    expect(active()).toContain("GOAD Light");
    await user.type(field(), "overview");
    expect(active()).toContain("Go to Overview");
    await user.keyboard("{Enter}");
    expect(run).toHaveBeenCalledWith("home");
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it("finds a lab by its category hint and keywords, not only its title", async () => {
    const user = userEvent.setup();
    render(<CommandPalette open onClose={vi.fn()} commands={cmds()} />);
    await user.type(field(), "active directory");
    // The results are a listbox's options (a screen reader hears which one is picked).
    const rows = within(screen.getByRole("dialog")).getAllByRole("option");
    expect(rows.map((r) => r.textContent)).toEqual([expect.stringContaining("GOAD Light")]);
  });

  it("titles each group once, above its first command", () => {
    render(<CommandPalette open onClose={vi.fn()} commands={cmds()} />);
    const text = screen.getByRole("dialog").textContent ?? "";
    expect(text.match(/Screens/g)).toHaveLength(1);
    expect(text.match(/Labs/g)).toHaveLength(1);
  });

  it("does nothing on Enter when no command matches", async () => {
    const run = vi.fn();
    const onClose = vi.fn();
    const user = userEvent.setup();
    render(<CommandPalette open onClose={onClose} commands={cmds(run)} />);
    await user.type(field(), "zzzz");
    await user.keyboard("{Enter}");
    expect(run).not.toHaveBeenCalled();
    expect(onClose).not.toHaveBeenCalled();
    expect(screen.getByText("No results")).toBeTruthy();
  });

  it("keeps the focus in the search field: Tab doesn't leave the palette, and Enter runs the picked result", async () => {
    // A combobox over a listbox: the results are options picked with the arrows, never focused.
    const run = vi.fn();
    render(<CommandPalette open onClose={vi.fn()} commands={cmds(run)} />);
    await waitFor(() => expect(document.activeElement).toBe(field()));
    fireEvent.keyDown(field(), { key: "Tab" });
    expect(document.activeElement).toBe(field());
    fireEvent.keyDown(field(), { key: "Tab", shiftKey: true });
    expect(document.activeElement).toBe(field());
    fireEvent.keyDown(field(), { key: "ArrowDown" });
    fireEvent.keyDown(field(), { key: "Enter" });
    expect(run).toHaveBeenCalledOnce();
  });

  // Bug: Enter that confirms an IME composition (Japanese is a shipped language) runs the
  // highlighted command while the player is still typing a query. Suggested fix
  // (command-palette.tsx onKey): `if (e.nativeEvent.isComposing || e.keyCode === 229) return;`
  // before the Enter branch.
  it("Enter confirming an IME composition in the search field runs a command", () => {
    const run = vi.fn();
    render(<CommandPalette open onClose={vi.fn()} commands={cmds(run)} />);
    fireEvent.keyDown(field(), { key: "Enter", isComposing: true, keyCode: 229 });
    expect(run).not.toHaveBeenCalled();
  });
});

describe("AppShell shortcuts", () => {
  it("toggles the palette closed with Ctrl+K pressed again while typing in it", async () => {
    onLinux();
    installTauri();
    const user = userEvent.setup();
    await shell();
    await user.keyboard("{Control>}k{/Control}");
    const dialog = screen.getByRole("dialog");
    await waitFor(() => expect(dialog.contains(document.activeElement)).toBe(true));
    await user.keyboard("cl");
    await user.keyboard("{Control>}k{/Control}");
    expect(screen.queryByRole("dialog")).toBeNull();
  });

  it("opens the palette fresh each time: the last query is gone", async () => {
    onLinux();
    installTauri();
    const user = userEvent.setup();
    await shell();
    await user.keyboard("{Control>}k{/Control}");
    await waitFor(() => expect(screen.getByRole("dialog").contains(document.activeElement)).toBe(true));
    await user.keyboard("cloud");
    await user.keyboard("{Escape}");
    await user.keyboard("{Control>}k{/Control}");
    expect(screen.getByRole("dialog").querySelector("input")!.value).toBe("");
  });

  it("types / into the palette's field instead of jumping to the lab search", async () => {
    installTauri();
    const user = userEvent.setup();
    await shell();
    await user.click(screen.getByRole("button", { name: /find/i }));
    const input = screen.getByRole("dialog").querySelector("input")!;
    await waitFor(() => expect(document.activeElement).toBe(input));
    await user.keyboard("/");
    await flush(30);
    expect(input.value).toBe("/");
    expect(current()).toContain("Overview");
    // The "Find a lab" action has "/" as its hint, so it matches.
    expect(screen.getByRole("dialog").textContent).toContain("Find a lab");
  });

  it("ignores / with a modifier held", async () => {
    installTauri();
    await shell();
    fireEvent.keyDown(window, { key: "/", altKey: true });
    fireEvent.keyDown(window, { key: "/", ctrlKey: true });
    await flush(30);
    expect(current()).toContain("Overview");
  });

  it("gives the focus back to the Find button when the palette closes", async () => {
    installTauri();
    const user = userEvent.setup();
    await shell();
    const find = screen.getByRole("button", { name: /find/i });
    await user.click(find);
    await waitFor(() => expect(screen.getByRole("dialog").contains(document.activeElement)).toBe(true));
    await user.keyboard("{Escape}");
    expect(document.activeElement).toBe(find);
  });

  it("names every button in the main window, so a screen reader can tell them apart", async () => {
    installTauri();
    await shell();
    const unnamed = screen
      .getAllByRole("button")
      .filter((b) => !(b.textContent ?? "").trim() && !b.getAttribute("aria-label") && !b.getAttribute("title") && !b.getAttribute("aria-labelledby"));
    expect(unnamed.map((b) => b.outerHTML.slice(0, 120))).toEqual([]);
  });

  // Bug: the shell's window keydown handler runs while a modal is open. With the quit guard (or
  // the launch confirmation) up and its button focused, "/" switches the screen behind the modal
  // and its requestAnimationFrame then focuses the lab search behind it, breaking the focus trap;
  // Ctrl+K opens the palette over it. Suggested fix (app-shell.tsx onKey): return early when
  // `document.activeElement?.closest('[aria-modal="true"]')` is set and is not the palette (or
  // track an "a modal is open" flag the dialogs set).
  it("/ switches the screen behind the quit guard while the dialog has the focus", async () => {
    installTauri();
    await shell();
    await act(() => emit("quit-blocked", 1));
    await flush();
    const dialog = screen.getByRole("alertdialog");
    expect(dialog.contains(document.activeElement)).toBe(true);
    fireEvent.keyDown(document.activeElement!, { key: "/" });
    await flush(50);
    expect(current()).toContain("Overview");
    expect(dialog.contains(document.activeElement)).toBe(true);
  });

  it("keeps / and Ctrl+K off while a modal is open, even with the focus outside it", async () => {
    onLinux();
    installTauri();
    await shell();
    await act(() => emit("quit-blocked", 1));
    await flush();
    const dialog = screen.getByRole("alertdialog");
    // A click on the page behind, or a dialog that didn't take the focus: it's on the body.
    (document.activeElement as HTMLElement | null)?.blur();
    expect(dialog.contains(document.activeElement)).toBe(false);
    fireEvent.keyDown(window, { key: "/" });
    fireEvent.keyDown(window, { key: "k", ctrlKey: true });
    await flush(50);
    expect(current()).toContain("Overview");
    expect(screen.queryByRole("dialog")).toBeNull();
  });

  // Bug: shortcutFor accepts Ctrl+K on macOS too, where the palette's shortcut is Cmd+K (the
  // sidebar shows "⌘K"). On macOS, Ctrl+K is the system's "delete to end of line" in every text
  // field, and in the attack box terminal the shell's kill-line: the shell takes it (with
  // preventDefault) and pops the palette over what the player was typing. Suggested fix
  // (shell-model.ts shortcutFor): match `mac ? e.metaKey : e.ctrlKey` for the palette, as the
  // Settings shortcut already does by platform.
  it("on macOS Ctrl+K in a text field opens the palette instead of reaching the field", async () => {
    onMac();
    installTauri();
    await shell();
    const input = document.createElement("input");
    document.body.appendChild(input);
    input.focus();
    fireEvent.keyDown(input, { key: "k", ctrlKey: true });
    expect(screen.queryByRole("dialog")).toBeNull();
    input.remove();
  });
});

describe("QuitGuard", () => {
  it("shows the latest count when another quit is blocked while it is open", async () => {
    installTauri();
    render(<QuitGuard />);
    await flush();
    await act(() => emit("quit-blocked", 1));
    await flush();
    await act(() => emit("quit-blocked", 2));
    await flush();
    expect(screen.getAllByRole("alertdialog")).toHaveLength(1);
    expect(screen.getByRole("alertdialog").textContent).toContain("2 labs are being worked on");
  });

  it("puts the focus on Keep waiting, the safe choice, and gives it back when closed", async () => {
    installTauri();
    const user = userEvent.setup();
    render(
      <>
        <button>Behind</button>
        <QuitGuard />
      </>,
    );
    await flush();
    const behind = screen.getByRole("button", { name: "Behind" });
    behind.focus();
    await act(() => emit("quit-blocked", 1));
    await flush();
    expect(document.activeElement?.textContent).toBe("Keep waiting");
    await user.keyboard("{Enter}");
    expect(screen.queryByRole("alertdialog")).toBeNull();
    expect(document.activeElement).toBe(behind);
  });

  it("disables both quit buttons while one is in flight, so a double click can't send two", async () => {
    let release: () => void = () => {};
    const calls = installTauri({ linger_quit: () => new Promise<void>((r) => (release = r)) });
    const user = userEvent.setup();
    render(<QuitGuard />);
    await flush();
    await act(() => emit("quit-blocked", 1));
    await flush();
    await user.click(screen.getByRole("button", { name: "Finish in background" }));
    await user.click(screen.getByRole("button", { name: "Finish in background" }));
    expect(screen.getByRole("button", { name: "Quit anyway" }).hasAttribute("disabled")).toBe(true);
    expect(commands(calls).filter((c) => c === "linger_quit" || c === "force_quit")).toEqual(["linger_quit"]);
    release();
  });

  // Bug: the quit guard is a modal alertdialog with no Escape handling: Escape does nothing, so a
  // keyboard player must Tab to "Keep waiting". Every other dialog (ConfirmDialog, the palette)
  // closes on Escape. Suggested fix (quit-guard.tsx): an onKeyDown on the dialog that calls
  // setCount(null) on Escape (keep waiting is the safe answer), as ConfirmDialog does.
  it("Escape doesn't dismiss the quit guard (keep waiting)", async () => {
    const calls = installTauri();
    const user = userEvent.setup();
    render(<QuitGuard />);
    await flush();
    await act(() => emit("quit-blocked", 1));
    await flush();
    await user.keyboard("{Escape}");
    expect(screen.queryByRole("alertdialog")).toBeNull();
    expect(commands(calls)).not.toContain("force_quit");
  });
});

describe("LaunchConfirm keyboard", () => {
  const request = { sessionId: "sess-1", repository: "cyber-courses/lab-s3", commit: "0123456789abcdef", target: "AWS sandbox" };

  it("puts the focus on Decline first, so Enter after a misclick never starts billable resources", async () => {
    const calls = installTauri({ confirm_launch: () => null });
    const user = userEvent.setup();
    render(<LaunchConfirm />);
    await flush();
    await act(() => emit("launch-confirm", request));
    await flush();
    expect(document.activeElement?.textContent).toBe("Decline");
    await user.keyboard("{Enter}");
    await flush();
    expect(calls.filter((c) => c.cmd === "confirm_launch").map((c) => c.args)).toEqual([{ sessionId: "sess-1", approve: false }]);
  });

  // Bug: like the quit guard, the launch confirmation has no Escape handling. Suggested fix
  // (launch-confirm.tsx): Escape answers `answer(false)` (or just closes when expired), the same
  // as Decline.
  it("Escape doesn't decline the cloud launch confirmation", async () => {
    const calls = installTauri({ confirm_launch: () => null });
    const user = userEvent.setup();
    render(<LaunchConfirm />);
    await flush();
    await act(() => emit("launch-confirm", request));
    await flush();
    await user.keyboard("{Escape}");
    await flush();
    expect(calls.filter((c) => c.cmd === "confirm_launch").map((c) => c.args)).toEqual([{ sessionId: "sess-1", approve: false }]);
    expect(screen.queryByRole("alertdialog")).toBeNull();
  });
});

describe("ConfirmDialog keyboard", () => {
  it("puts the focus on Cancel, and Escape cancels without reaching the page's own Escape", async () => {
    const onCancel = vi.fn();
    const onConfirm = vi.fn();
    const pageEscape = vi.fn();
    window.addEventListener("keydown", pageEscape);
    const user = userEvent.setup();
    render(
      <ConfirmDialog title="Delete?" confirmLabel="Delete" onConfirm={onConfirm} onCancel={onCancel}>
        Gone for good.
      </ConfirmDialog>,
    );
    expect(document.activeElement?.textContent).toBe("Cancel");
    await user.keyboard("{Escape}");
    expect(onCancel).toHaveBeenCalledTimes(1);
    expect(onConfirm).not.toHaveBeenCalled();
    expect(pageEscape).not.toHaveBeenCalled();
    window.removeEventListener("keydown", pageEscape);
  });

  it("keeps Tab between Cancel and the confirm button", () => {
    render(
      <ConfirmDialog title="Delete?" confirmLabel="Delete" onConfirm={vi.fn()} onCancel={vi.fn()}>
        Gone for good.
      </ConfirmDialog>,
    );
    const [cancel, del] = screen.getAllByRole("button");
    fireEvent.keyDown(del, { key: "Tab" });
    expect(document.activeElement).toBe(cancel);
    fireEvent.keyDown(cancel, { key: "Tab", shiftKey: true });
    expect(document.activeElement).toBe(del);
  });
});

describe("UpdateBanner retry", () => {
  it("lets the player try again after a failed install, and relaunches when it works", async () => {
    let tries = 0;
    updater.pending = {
      version: "9.9.9",
      downloadAndInstall: async () => {
        if (++tries === 1) throw new Error("signature mismatch");
      },
    };
    // No lab deploying: an update waits for deploys to finish.
    const calls = installTauri({ restart_app: () => null, deploying_labs: () => [] });
    const user = userEvent.setup();
    render(<UpdateBanner />);
    await flush();
    expect(screen.getByRole("status").textContent).toContain("9.9.9");
    await user.click(screen.getByRole("button", { name: "Update" }));
    await flush();
    expect(screen.getByRole("status").textContent).toContain("Update failed");
    expect(commands(calls)).not.toContain("restart_app");
    await user.click(screen.getByRole("button", { name: "Update" }));
    await flush();
    expect(tries).toBe(2);
    expect(commands(calls)).toContain("restart_app");
  });
});

// @vitest-environment jsdom
import type React from "react";
import { act, render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { ImportLab } from "@/features/labs/import-lab";
import { RowAction } from "@/features/labs/lab-row-actions";
import { SignInBanner } from "@/features/labs/labs-list";
import { useLabActions } from "@/features/labs/use-lab-actions";
import type { Lab } from "@/features/labs/use-labs";
import { getDeploySnapshot } from "@/features/labs/runs/deploy-runs";
import { Toaster } from "@/components/ui/toaster";
import { must, toastsShown } from "@/components/test-support";
import { renderLabPage } from "@/features/labs/lab-detail/lab-detail-test-kit";
import { containerMachine, labStatus, testLab, upStatus } from "@/features/labs/labs-test-kit";
import { installTauri, rejects } from "@/test/tauri";
import { flush } from "@/test/ui";

// The failures a player can run into around starting a lab that the earlier suites left
// untested: each one is said where the player is (a toast), and nothing crashes or hangs.

// The dialog plugin: the folder the player picks, or a picker that fails.
const picker = vi.hoisted(() => ({ answer: (): Promise<unknown> => Promise.resolve(null) }));
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: () => picker.answer() }));

const idle = { deploying_labs: () => [], active_operations: () => [] };
const blind = testLab("lab-sqli", { title: "Blind orders", runtime: { runtime: "DOCKER", architectures: ["x86_64"], providers: [] } });
const goat: Lab = testLab("lab-goat", {
  title: "EC2 SSRF",
  runtime: { runtime: "CLOUD", architectures: [], providers: ["aws"], repository: "CyberCTF/cloudgoat-ec2-ssrf", commit: "0".repeat(40) },
});

/** The failure toasts on screen, as text. */
const alerts = () => screen.queryAllByRole("alert").map((a) => a.textContent);

/** The lab actions hook, rendered with the Toaster so what it tells shows. */
function renderActions() {
  let actions: ReturnType<typeof useLabActions> | null = null;
  function Probe() {
    actions = useLabActions(() => {}, []);
    return null;
  }
  render(
    <>
      <Probe />
      <Toaster />
    </>,
  );
  return () => must(actions, "the lab actions");
}

const rowProps = { running: false, parked: false as const, busy: false, doing: null, setup: null, connectCloud: false, loggedIn: true, onStop: vi.fn() };

beforeEach(() => {
  toastsShown();
});
afterEach(() => {
  vi.restoreAllMocks();
  picker.answer = () => Promise.resolve(null);
});

/** Renders the Import button with the toaster and clicks it: the picker answers as `picker` says. */
async function importFromPicker() {
  const user = userEvent.setup();
  render(
    <>
      <ImportLab />
      <Toaster />
    </>,
  );
  await user.click(screen.getByRole("button", { name: /Import a lab/ }));
  await flush();
}

describe("importing a local lab", () => {
  it("says the import failed when the folder picker fails, and offers to import again", async () => {
    picker.answer = () => Promise.reject(new Error("no dialog here"));
    installTauri();
    await importFromPicker();
    expect(alerts().join("\n")).toContain("Couldn't import that lab");
    expect(alerts().join("\n")).toContain("no dialog here");
    expect(screen.getByRole("button", { name: /Import a lab/ }).hasAttribute("disabled")).toBe(false);
  });

  it("says the import failed when the lab can't start, with the reason, and is ready for another", async () => {
    picker.answer = () => Promise.resolve("/Users/player/labs/mine");
    const calls = installTauri({ lab_launch_local: rejects({ code: "other", message: "no isoloom.yml in /Users/player/labs/mine" }) });
    await importFromPicker();
    expect(calls.find((c) => c.cmd === "lab_launch_local")?.args).toMatchObject({ path: "/Users/player/labs/mine" });
    expect(alerts().join("\n")).toContain("Couldn't import that lab");
    expect(alerts().join("\n")).toContain("no isoloom.yml");
    expect(document.body.textContent).not.toContain("Importing…");
    expect(screen.getByRole("button", { name: /Import a lab/ }).hasAttribute("disabled")).toBe(false);
  });

  it("starts nothing when the player cancels the picker", async () => {
    const calls = installTauri();
    const user = userEvent.setup();
    render(<ImportLab />);
    await user.click(screen.getByRole("button", { name: /Import a lab/ }));
    await flush();
    expect(calls.map((c) => c.cmd)).not.toContain("lab_launch_local");
  });
});

describe("an operation the backend refused", () => {
  it("says another operation is still running, as a refusal and not a failed start", async () => {
    installTauri({ ...idle, lab_launch: rejects({ code: "alreadyInProgress", message: "lab-sqli: a launch is already in progress" }) });
    const actions = renderActions();
    await act(() => actions().launch(blind, null, undefined, null, { ports: "default" }));
    expect(alerts().join("\n")).toContain("Another operation is still running on this lab");
    expect(alerts().join("\n")).not.toContain("Couldn’t start");
    // Nothing ran, so no run is left behind reading as a failed deploy.
    expect(getDeploySnapshot().runs[blind.id]).toBeUndefined();
  });
});

describe("a cloud lab started without an account", () => {
  it("says to pick one of the player's cloud accounts", async () => {
    installTauri(idle);
    const actions = renderActions();
    await act(() => actions().launch(goat, null, undefined, null));
    const toast = must(
      alerts().find((a) => a.includes("Couldn’t start EC2 SSRF")),
      "the failed start toast",
    );
    expect(toast).toContain("pick one of your cloud accounts to start it on");
  });
});

describe("Reset after a stop that failed", () => {
  it("says the reset stopped and doesn't start the lab again", async () => {
    installTauri(idle);
    const user = userEvent.setup();
    // Resolves to the failure, as useLabActions' stop does when the removal fails.
    const onStop = vi.fn(() => Promise.resolve("Couldn't remove network lab-sqli_default: has active endpoints"));
    const h = renderLabPage(blind, { status: upStatus([containerMachine("api"), containerMachine("db", "exited")]), onStop });
    render(<Toaster />);
    await flush(50);
    expect(document.body.textContent).toContain("db is down");
    await user.click(screen.getByRole("button", { name: /Reset lab/ }));
    await flush(50);
    expect(onStop).toHaveBeenCalledTimes(1);
    expect(h.onStart).not.toHaveBeenCalled();
    expect(alerts().join("\n")).toContain("Reset stopped: the lab couldn't be removed, so it wasn't started again");
    expect(alerts().join("\n")).toContain("has active endpoints");
    // The banner is back to its resting state, ready for another try.
    expect(screen.getByRole("button", { name: /Reset lab/ }).hasAttribute("disabled")).toBe(false);
  });
});

describe("the cloud account setup can't open", () => {
  it("says so from a lab row's Connect button", async () => {
    installTauri({ server_open_setup: rejects({ code: "other", message: "window already closing" }) });
    const user = userEvent.setup();
    render(
      <>
        <RowAction {...rowProps} setup="Connect an AWS account" connectCloud />
        <Toaster />
      </>,
    );
    await user.click(screen.getByRole("button", { name: /Connect/ }));
    await flush();
    expect(alerts().join("\n")).toContain("Couldn't open the cloud account setup");
    expect(alerts().join("\n")).toContain("window already closing");
  });

  it("says so from the lab page's Connect an account button", async () => {
    installTauri({ ...idle, server_list: () => ({ default: null, hosts: [] }), server_open_setup: rejects({ code: "other", message: "webview denied" }) });
    const user = userEvent.setup();
    renderLabPage(goat, { status: labStatus() });
    render(<Toaster />);
    await flush(50);
    await user.click(screen.getByRole("button", { name: /Connect an account on AWS/ }));
    await flush();
    expect(alerts().join("\n")).toContain("Couldn't open the cloud account setup");
    expect(alerts().join("\n")).toContain("webview denied");
  });
});

describe("a sign-in that can't start", () => {
  const onLogin = () => vi.fn(() => Promise.reject(new Error("browser refused the link")));

  /** Renders `el` with the toaster and clicks its Sign in button. */
  async function signInFrom(el: React.ReactElement) {
    const user = userEvent.setup();
    render(
      <>
        {el}
        <Toaster />
      </>,
    );
    await user.click(screen.getByRole("button", { name: /Sign in/ }));
    await flush();
  }

  it("is said from a lab row", async () => {
    installTauri();
    await signInFrom(<RowAction {...rowProps} loggedIn={false} onLogin={onLogin()} />);
    expect(alerts().join("\n")).toContain("Couldn't start signing in");
    expect(alerts().join("\n")).toContain("browser refused the link");
  });

  it("is said from the labs list's sign-in banner", async () => {
    installTauri();
    await signInFrom(<SignInBanner onLogin={onLogin()} />);
    expect(alerts().join("\n")).toContain("Couldn't start signing in");
  });

  it("is said from the lab page's header", async () => {
    installTauri(idle);
    const user = userEvent.setup();
    renderLabPage(blind, { status: labStatus(), loggedIn: false, onLogin: onLogin() });
    render(<Toaster />);
    await flush(50);
    await user.click(screen.getByRole("button", { name: /Sign in to start/ }));
    await flush();
    expect(alerts().join("\n")).toContain("Couldn't start signing in");
    expect(alerts().join("\n")).toContain("browser refused the link");
  });
});

describe("a cloud lab whose cost preview can't be read", () => {
  it("says the lab gives no estimate instead of a cost", async () => {
    installTauri({ ...idle, lab_cloud_preview: rejects({ code: "other", message: "git fetch failed" }) });
    const user = userEvent.setup();
    renderLabPage(goat, { status: labStatus() });
    await flush(50);
    expect(document.body.textContent).toContain("Billed to your cloud account while it runs.");
    expect(document.body.textContent).not.toContain("$0.02");
    await user.click(screen.getByRole("button", { name: /Start lab/ }));
    const dialog = await screen.findByRole("dialog");
    expect(within(dialog).getByText("This lab gives no cost estimate.")).toBeTruthy();
  });
});

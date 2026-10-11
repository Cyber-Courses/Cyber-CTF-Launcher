// @vitest-environment jsdom
import { act, render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { LabDetail } from "@/features/labs/lab-detail";
import type { Lab } from "@/features/labs/use-labs";
import type { LabStatus } from "@/lib/tauri";
import { installTauri } from "@/test/tauri";

const goat: Lab = {
  id: "lab-goat",
  slug: "cloudgoat-ec2-ssrf",
  title: "EC2 SSRF",
  description: "From a low-privilege IAM user to a Lambda function.",
  question: null,
  difficulty: 2,
  category: "Cloud",
  runtime: { runtime: "CLOUD", architectures: [], providers: ["aws"], repository: "CyberCTF/cloudgoat-ec2-ssrf", commit: "0".repeat(40) },
  skills: [],
};

const status = (p: Partial<LabStatus>): LabStatus => ({
  running: false,
  parked: null,
  machines: [],
  networks: [],
  url: null,
  host: null,
  expiresAt: null,
  place: null,
  provider: null,
  attacker: null,
  outputs: [],
  message: null,
  ...p,
});

const flush = () =>
  act(async () => {
    await new Promise((r) => setTimeout(r, 30));
  });

function renderGoat(props: Partial<Parameters<typeof LabDetail>[0]> = {}) {
  const handlers = { onBack: vi.fn(), onStart: vi.fn(), onStop: vi.fn(), onPark: vi.fn(), onResume: vi.fn(), onProvision: vi.fn(), onLogin: vi.fn() };
  render(
    <LabDetail lab={goat} busy={false} logs={[]} times={[]} loggedIn hostArch="x86_64" readyVms={["virtualbox"]} dockerRunning {...handlers} {...props} />,
  );
  return handlers;
}

const idle = { deploying_labs: () => [], active_operations: () => [] };

describe("LabDetail, a cloud lab", () => {
  it("starts on one of the player's accounts of its cloud, its cost said first", async () => {
    installTauri(idle);
    const user = userEvent.setup();
    const h = renderGoat({ status: status({}) });
    await flush();
    expect(screen.getByText(/Start the lab to create its cloud services/)).toBeTruthy();
    expect(screen.getByText("Billed to you: about $0.02 an hour, by the lab's estimate.")).toBeTruthy();
    await user.click(screen.getByRole("button", { name: /start lab/i }));
    const dialog = await screen.findByRole("dialog");
    // Cloud accounts only: nothing runs on this machine.
    expect(within(dialog).queryByText("This machine")).toBeNull();
    expect(within(dialog).getByText("Billed to your cloud account.")).toBeTruthy();
    expect(within(dialog).getByText("It lets in your public IP only.")).toBeTruthy();
    const start = within(dialog)
      .getAllByRole("button")
      .filter((b) => /^start$/i.test(b.textContent?.trim() ?? ""));
    await user.click(start[0]);
    expect(h.onStart).toHaveBeenCalledWith({ kind: "host", id: "aws1" });
  });

  it("with no account of its cloud, offers to connect one", async () => {
    const calls = installTauri({ ...idle, server_list: () => ({ default: null, hosts: [] }) });
    const user = userEvent.setup();
    renderGoat({ status: status({}) });
    await flush();
    await user.click(screen.getByRole("button", { name: "Connect an account on AWS" }));
    expect(calls.some((c) => c.cmd === "server_open_setup" && c.args?.kind === "cloud")).toBe(true);
  });

  it("shows what a running one exposes, its secrets masked until asked", async () => {
    installTauri(idle);
    const user = userEvent.setup();
    renderGoat({
      status: status({
        running: true,
        host: "AWS sandbox",
        place: "cloud",
        provider: "aws",
        outputs: [
          { name: "access_key_id", value: "AKIAEXAMPLE", sensitive: false },
          { name: "secret_key", value: "s3cr3t", sensitive: true },
        ],
        message: "Sign in as Solus.",
      }),
    });
    await flush();
    expect(screen.getByText("Sign in as Solus.")).toBeTruthy();
    expect(screen.getByText("AKIAEXAMPLE")).toBeTruthy();
    expect(screen.queryByText("s3cr3t")).toBeNull();
    await user.click(screen.getByRole("button", { name: "Show secret_key" }));
    expect(screen.getByText("s3cr3t")).toBeTruthy();
    await user.click(screen.getByRole("button", { name: "Hide secret_key" }));
    expect(screen.queryByText("s3cr3t")).toBeNull();
    // No attack box: the player works from their own machine.
    expect(screen.queryByText("Attack box")).toBeNull();
    expect(screen.getByText("Estimated cost")).toBeTruthy();
  });

  it("says when a cut-off start left services behind", async () => {
    installTauri(idle);
    renderGoat({ status: status({ host: "AWS sandbox", provider: "aws" }) });
    await flush();
    expect(screen.getByText(/A previous start left cloud services behind/)).toBeTruthy();
  });

  it("running with nothing to show, says so", async () => {
    installTauri(idle);
    renderGoat({ status: status({ running: true, host: "AWS sandbox", place: "cloud", provider: "aws" }) });
    await flush();
    expect(screen.getByText(/It exposes nothing to show here/)).toBeTruthy();
  });
});

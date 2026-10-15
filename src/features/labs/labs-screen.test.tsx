// @vitest-environment jsdom
import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { Labs } from "@/features/labs/labs-screen";
import { answer } from "@/lib/dev-mock";
import { readyReport } from "@/test/fixtures";
import { installTauri, type Handler } from "@/test/tauri";
import { flush } from "@/test/ui";
import { rowTitles } from "@/features/labs/labs-test-kit";

// The Labs list: what it shows for the catalogue and each filter, and what a row does.

const props = {
  loggedIn: true,
  authReady: true,
  onLogin: vi.fn(async () => {}),
  hostArch: "x86_64",
  report: { ...readyReport(), arch: "x86_64" },
  openLab: { slug: null, tick: 0 },
  onDetailChange: vi.fn(),
};
const idle: Record<string, Handler> = { deploying_labs: () => [], active_operations: () => [] };
const titles = rowTitles;

describe("Labs list", () => {
  it("pins running labs on top and sorts the rest by title, with a summary", async () => {
    installTauri(idle);
    render(<Labs {...props} />);
    await flush(80);
    expect(screen.getByText("6 in the catalogue · 1 running on this machine · 1 solved")).toBeTruthy();
    expect(screen.getByText("Running now")).toBeTruthy();
    expect(screen.getByText("1 lab")).toBeTruthy();
    expect(titles()).toEqual(["Invoice portal API", "Blind orders", "EC2 SSRF", "Exposed dashboard", "GOAD Light", "Public backup"]);
  });

  it("filters by status, runtime, level and search, and says when nothing matches", async () => {
    installTauri(idle);
    const user = userEvent.setup();
    render(<Labs {...props} />);
    await flush(80);
    const group = (name: string) => screen.getByRole("radiogroup", { name });
    await user.click(within(group("Status")).getByRole("radio", { name: "Running" }));
    expect(titles()).toEqual(["Invoice portal API"]);
    await user.click(within(group("Status")).getByRole("radio", { name: "Solved" }));
    expect(titles()).toEqual(["Public backup"]);
    await user.click(within(group("Status")).getByRole("radio", { name: "To do" }));
    expect(titles()).not.toContain("Public backup");
    await user.click(within(group("Status")).getByRole("radio", { name: "All" }));
    await user.click(within(group("Runtime")).getByRole("radio", { name: "VM" }));
    expect(titles()).toEqual(["GOAD Light"]);
    // Cloud: the cloud-services lab, and the labs a cloud account can run.
    await user.click(within(group("Runtime")).getByRole("radio", { name: "Cloud" }));
    expect(titles()).toEqual(["Invoice portal API", "EC2 SSRF", "Public backup"]);
    await user.click(within(group("Runtime")).getByRole("radio", { name: "Any runtime" }));
    await user.click(within(group("Level")).getByRole("radio", { name: "Hard" }));
    expect(titles()).toEqual(["GOAD Light"]);
    await user.click(within(group("Level")).getByRole("radio", { name: "Any level" }));
    await user.type(screen.getByRole("textbox", { name: "Search labs and skills" }), "kerberoast");
    expect(titles()).toEqual(["GOAD Light"]);
    await user.type(screen.getByRole("textbox", { name: "Search labs and skills" }), "zzz");
    expect(screen.getByText("No labs match")).toBeTruthy();
  });

  it("says the catalogue can't be reached", async () => {
    installTauri({
      ...idle,
      api_query: (a) => {
        if (String(a?.operation).includes("labs")) throw new Error("offline");
        return {};
      },
    });
    render(<Labs {...props} loggedIn={false} />);
    await flush(80);
    expect(screen.getByText("Can’t reach the lab catalogue")).toBeTruthy();
  });

  it("offers a Retry when the catalogue can't be reached, which loads it again", async () => {
    let offline = true;
    const calls = installTauri({
      ...idle,
      api_query: (a) => {
        if (String(a?.operation) === "labs" && offline) throw new Error("offline");
        return answer("api_query", a);
      },
    });
    const user = userEvent.setup();
    render(<Labs {...props} />);
    await flush(80);
    expect(screen.getByText("Can’t reach the lab catalogue")).toBeTruthy();
    const asked = () => calls.filter((c) => c.cmd === "api_query" && c.args?.operation === "labs").length;
    const before = asked();
    offline = false;
    await user.click(screen.getByRole("button", { name: "Retry" }));
    await flush(80);
    expect(asked()).toBeGreaterThan(before);
    expect(screen.queryByText("Can’t reach the lab catalogue")).toBeNull();
  });

  it("signed out: one sign-in banner above the list, no Sign in per row, no solved count", async () => {
    installTauri(idle);
    const onLogin = vi.fn(async () => {});
    const user = userEvent.setup();
    render(<Labs {...props} loggedIn={false} onLogin={onLogin} />);
    await flush(80);
    expect(screen.getByText("Sign in to start labs")).toBeTruthy();
    const signIn = screen.getAllByRole("button", { name: "Sign in" });
    expect(signIn).toHaveLength(1);
    expect(document.body.textContent).not.toMatch(/solved/);
    await user.click(signIn[0]);
    expect(onLogin).toHaveBeenCalledTimes(1);
  });

  it("says when no lab is published yet", async () => {
    installTauri({ ...idle, api_query: (a) => (String(a?.operation).includes("labs") ? { labs: [] } : {}), running_labs: () => [] });
    render(<Labs {...props} loggedIn={false} />);
    await flush(80);
    expect(screen.getByText("No labs published yet")).toBeTruthy();
    // Signed out, nothing is tracked: no solved count.
    expect(screen.getByText("0 in the catalogue · 0 running on this machine")).toBeTruthy();
  });

  it("shows placeholder rows while the catalogue loads", async () => {
    installTauri({ ...idle, api_query: () => new Promise(() => {}) });
    render(<Labs {...props} loggedIn={"loading" as unknown as boolean} />);
    await flush();
    expect(screen.getByText("Hands-on labs that run on this machine, your servers or your cloud.")).toBeTruthy();
    expect(titles()).toEqual([]);
  });

  it("waits for the catalogue before opening a lab from a link, then opens it", async () => {
    let release!: (v: unknown) => void;
    installTauri({
      ...idle,
      api_query: (a) => {
        const q = String(a?.operation);
        if (!q.includes("labs")) return {};
        return new Promise((r) => (release = r));
      },
    });
    const onDetailChange = vi.fn();
    render(<Labs {...props} loggedIn={"slow" as unknown as boolean} openLab={{ slug: "goad-light", tick: 1 }} onDetailChange={onDetailChange} />);
    await flush();
    // A spinner, never the list.
    expect(screen.queryByText("Labs")).toBeNull();
    const { answer } = await import("@/lib/dev-mock");
    release(answer("api_query", { operation: "labs" }));
    await flush(80);
    expect(onDetailChange).toHaveBeenCalledWith("GOAD Light");
    expect(screen.getByRole("heading", { name: "GOAD Light" })).toBeTruthy();
  });

  it("opens a lab from its row and comes back to the list", async () => {
    installTauri(idle);
    const user = userEvent.setup();
    const onDetailChange = vi.fn();
    render(<Labs {...props} onDetailChange={onDetailChange} />);
    await flush(80);
    await user.click(screen.getByRole("button", { name: "Open Blind orders" }));
    await flush();
    expect(screen.getByRole("heading", { name: "Blind orders" })).toBeTruthy();
    expect(onDetailChange).toHaveBeenLastCalledWith("Blind orders");
    await user.click(screen.getByRole("button", { name: /All labs/ }));
    expect(onDetailChange).toHaveBeenLastCalledWith(null);
    expect(screen.getByText("Running now")).toBeTruthy();
  });

  it("follows navigation to another lab and back to the list", async () => {
    installTauri(idle);
    const { rerender } = render(<Labs {...props} />);
    await flush(80);
    rerender(<Labs {...props} openLab={{ slug: "goad-light", tick: 1 }} />);
    await flush();
    expect(screen.getByRole("heading", { name: "GOAD Light" })).toBeTruthy();
    rerender(<Labs {...props} openLab={{ slug: null, tick: 2 }} />);
    await flush();
    expect(screen.getByText("Running now")).toBeTruthy();
  });

  it("asks before stopping a running lab from its row, then stops it", async () => {
    const calls = installTauri(idle);
    const user = userEvent.setup();
    render(<Labs {...props} />);
    await flush(80);
    await user.click(screen.getByRole("button", { name: "Stop" }));
    await user.click(screen.getByRole("button", { name: "Remove" }));
    await flush(50);
    expect(calls.find((c) => c.cmd === "lab_stop")?.args).toMatchObject({ id: "lab-invoice", runtime: "DOCKER" });
  });

  it("points a fresh machine at setup for the labs it can't run", async () => {
    installTauri({ ...idle, server_list: () => ({ default: null, hosts: [] }) });
    const report = { ...readyReport(), arch: "x86_64", dockerRunning: false };
    render(<Labs {...props} report={report} />);
    await flush(80);
    expect(screen.getAllByRole("button", { name: /Set up/ }).length).toBeGreaterThan(0);
  });
});

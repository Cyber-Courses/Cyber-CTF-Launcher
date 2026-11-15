// @vitest-environment jsdom
import { render, renderHook, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { FailureNotice, StatusReadNotice, useOpenServer } from "@/features/labs/lab-detail/failure-notice";
import { installTauri } from "@/test/tauri";
import { flush } from "@/test/ui";

// tests/resilience.md N4: a revoked Proxmox token or a changed ESXi password, said as the server
// refusing the saved credentials, with the way to its settings.

const refused = "Cyber CTF can't sign in to Home lab: its token was revoked or its password changed. Update it in Servers.";

describe("a server refusing the lab's credentials", () => {
  it("says so under the failed start, with its settings a click away", async () => {
    const open = vi.fn();
    render(<FailureNotice title="Blind orders" op="launch" message={refused} kind="serverAuth" stillUp={false} onOpenServer={open} onDismiss={() => {}} />);
    expect(screen.getByText(refused)).toBeTruthy();
    expect(screen.getByText(/refused the credentials Cyber CTF has for it/)).toBeTruthy();
    await userEvent.click(screen.getByRole("button", { name: "Open server settings" }));
    expect(open).toHaveBeenCalledOnce();
  });

  it("offers no settings for another failure, or a lab on no saved server", () => {
    render(<FailureNotice title="Blind orders" op="launch" message="boom" kind="tool" stillUp={false} onOpenServer={() => {}} onDismiss={() => {}} />);
    render(<FailureNotice title="Blind orders" op="launch" message={refused} kind="serverAuth" stillUp={false} onDismiss={() => {}} />);
    expect(screen.queryByRole("button", { name: "Open server settings" })).toBeNull();
  });

  it("says so when the lab's state can't be read, and opens the server's own settings", async () => {
    const calls = installTauri();
    const { result } = renderHook(() => useOpenServer("pve-1"));
    render(
      <StatusReadNotice
        title="Blind orders"
        error={{ code: "serverAuthFailed", args: { server: "Home lab" }, message: refused }}
        onOpenServer={result.current}
      />,
    );
    expect(screen.getByText(refused)).toBeTruthy();
    await userEvent.click(screen.getByRole("button", { name: "Open server settings" }));
    await flush();
    expect(calls.find((c) => c.cmd === "server_open_setup")?.args).toEqual({ id: "pve-1", kind: "server" });
    // Another read failure: no settings to open.
    render(<StatusReadNotice title="Other" error={{ code: "other", message: "Docker isn't running" }} onOpenServer={result.current} />);
    expect(screen.getAllByRole("button", { name: "Open server settings" })).toHaveLength(1);
    expect(renderHook(() => useOpenServer(null)).result.current).toBeUndefined();
  });
});

describe("a failed operation's id (#379)", () => {
  it("is shown under the failure for the player to quote, and only when there is one", () => {
    render(<FailureNotice title="Blind orders" op="launch" message="boom" kind={null} operation="op-0123abcd" stillUp={false} onDismiss={() => {}} />);
    expect(screen.getByText("Operation op-0123abcd: quote it when you ask for help.")).toBeTruthy();
  });

  it("comes with the failure of an operation the action followed through the run events", async () => {
    const { operationOf, takeIn, awaitOperation } = await import("@/lib/tauri/run-events");
    installTauri({ run_events: () => [] });
    takeIn([{ cursor: 1, event: { kind: "started", operation: "op-9", labId: "web", op: "launch", machine: null, runtime: null, deploying: true, at: 0 } }]);
    const failed = awaitOperation("op-9", () => {});
    takeIn([{ cursor: 2, event: { kind: "ended", operation: "op-9", labId: "web", op: "launch", error: { code: "other", message: "boom" } } }]);
    const error: unknown = await failed.catch((e: unknown) => e);
    expect(operationOf(error)).toBe("op-9");
    expect(operationOf({ code: "other", message: "a refusal" })).toBeNull();
  });
});

// @vitest-environment jsdom
import { useState } from "react";
import { act, fireEvent, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";
import { ErrorScreen } from "@/components/error-screen";
import { Sidebar } from "@/components/shell/sidebar";
import { ShellHeader } from "@/components/shell/shell-header";
import { CopyValue } from "@/components/ui/copy-value";
import { LogConsole } from "@/components/ui/log-console";
import { Meter } from "@/components/ui/meter";
import { NoticePanel } from "@/components/ui/notice-panel";
import { RadioList, RadioRow } from "@/components/ui/radio-row";
import { Sparkline } from "@/components/ui/sparkline";
import { Tip } from "@/components/ui/tip";
import { Toaster, toast } from "@/components/ui/toaster";
import { freshReport, readyReport, stoppedDockerReport } from "@/test/fixtures";
import { installTauri } from "@/test/tauri";
import { flush } from "@/test/ui";

afterEach(() => {
  vi.useRealTimers();
  vi.restoreAllMocks();
});

const clipboard = (writeText: (s: string) => Promise<void>) => Object.defineProperty(navigator, "clipboard", { value: { writeText }, configurable: true });

describe("Sidebar", () => {
  const props = {
    tab: "home" as const,
    isMac: false,
    version: null,
    labCount: 0,
    activeLabs: [],
    auth: null,
    onFind: vi.fn(),
    onNavigate: vi.fn(),
    onOpenSettings: vi.fn(),
    onAuthChange: vi.fn(),
  };

  it("shows Ctrl shortcuts off macOS and ⌘ ones on it", () => {
    installTauri();
    const { rerender } = render(<Sidebar {...props} />);
    expect(screen.getByText("Ctrl K")).toBeTruthy();
    expect(screen.getByText("Ctrl ,")).toBeTruthy();
    expect(document.querySelectorAll("[data-tauri-drag-region]")).toHaveLength(1);
    rerender(<Sidebar {...props} isMac version="1.4.0" labCount={12} />);
    expect(screen.getByText("⌘K")).toBeTruthy();
    expect(screen.getByText("⌘,")).toBeTruthy();
    expect(screen.getByText("v1.4.0")).toBeTruthy();
    expect(screen.getByText("12")).toBeTruthy();
    // The macOS titlebar band.
    expect(document.querySelectorAll("[data-tauri-drag-region]")).toHaveLength(2);
  });

  it("marks the open screen and navigates", async () => {
    installTauri();
    const onNavigate = vi.fn();
    const onFind = vi.fn();
    const onOpenSettings = vi.fn();
    const user = userEvent.setup();
    render(<Sidebar {...props} tab="cloud" onNavigate={onNavigate} onFind={onFind} onOpenSettings={onOpenSettings} />);
    expect(screen.getByRole("button", { current: "page" }).textContent).toContain("Cloud");
    await user.click(screen.getByRole("button", { name: /^Servers/ }));
    expect(onNavigate).toHaveBeenCalledWith("server");
    await user.click(screen.getByRole("button", { name: /find/i }));
    expect(onFind).toHaveBeenCalled();
    await user.click(screen.getByRole("button", { name: /^Settings/ }));
    expect(onOpenSettings).toHaveBeenCalled();
  });

  it("lists the labs running, with what a busy one is doing", async () => {
    installTauri();
    const onNavigate = vi.fn();
    const user = userEvent.setup();
    render(
      <Sidebar
        {...props}
        onNavigate={onNavigate}
        activeLabs={[
          { id: "a", slug: "invoice-portal-api", title: "Invoice portal API", category: "Web", op: null },
          {
            id: "b",
            slug: "goad-light",
            title: "GOAD Light",
            category: "AD",
            op: { labId: "b", op: "provision", machine: "isoloom-DC01", step: "TASK [users]" },
          },
          { id: "c", slug: "blind", title: "Blind orders", category: "Web", op: { labId: "c", op: "stop", machine: null, step: null } },
        ]}
      />,
    );
    expect(screen.getByText("Running")).toBeTruthy();
    expect(screen.getByText(/DC01/)).toBeTruthy();
    expect(screen.getByText("TASK [users]")).toBeTruthy();
    expect(screen.getByText(/stopping/i)).toBeTruthy();
    await user.click(screen.getByText("GOAD Light"));
    expect(onNavigate).toHaveBeenCalledWith("labs", "goad-light");
  });
});

describe("ShellHeader", () => {
  it("shows the breadcrumb and the engine this machine runs labs on", () => {
    render(<ShellHeader crumbs={["Labs", "GOAD Light"]} report={readyReport()} checkError={null} />);
    expect(screen.getByText("GOAD Light").tagName).toBe("B");
    expect(screen.getByText("Labs")).toBeTruthy();
    expect(document.body.textContent).toMatch(/Docker Desktop/);
  });

  it("says Docker isn't running, the check failed, or is still checking", () => {
    const { rerender } = render(<ShellHeader crumbs={["Overview"]} report={{ ...stoppedDockerReport(), vmProviders: [] }} checkError={null} />);
    expect(document.body.textContent).toMatch(/not running/i);
    rerender(<ShellHeader crumbs={["Overview"]} report={null} checkError="boom" />);
    expect(document.body.textContent).toMatch(/check failed|couldn't/i);
    rerender(<ShellHeader crumbs={["Overview"]} report={null} checkError={null} />);
    expect(document.body.textContent).toMatch(/checking/i);
    rerender(<ShellHeader crumbs={["Overview"]} report={freshReport("linux")} checkError={null} />);
    expect(document.body.textContent).toMatch(/not running/i);
  });
});

describe("Toaster", () => {
  it("shows the last three toasts, hides them after a while, and dismisses on click", async () => {
    vi.useFakeTimers();
    installTauri();
    render(<Toaster />);
    // The live region is there before any toast, so a screen reader is already watching it.
    const region = document.querySelector("[aria-live=polite]");
    expect(region).toBeTruthy();
    expect(screen.queryByRole("status")).toBeNull();
    act(() => {
      toast("one");
      toast("two", "details");
      toast("three");
      toast("four", "it broke", "fail");
    });
    const titles = (role: "status" | "alert") => screen.queryAllByRole(role).map((s) => s.querySelector("p")?.textContent);
    // A failure is an alert (announced at once), the others statuses.
    expect(titles("status")).toEqual(["two", "three"]);
    expect(titles("alert")).toEqual(["four"]);
    expect(screen.getByText("details")).toBeTruthy();
    act(() => vi.advanceTimersByTime(4000));
    // An ordinary toast goes after 4 s, a failure stays 7 s.
    expect(titles("status")).toEqual([]);
    expect(titles("alert")).toEqual(["four"]);
    fireEvent.click(screen.getByRole("button", { name: /dismiss/i }));
    expect(screen.queryByRole("alert")).toBeNull();
    // Empty again, the region stays mounted.
    expect(document.querySelector("[aria-live=polite]")).toBe(region);
  });

  it("stays under modals (z-50) so a toast never covers a dialog's buttons", () => {
    installTauri();
    render(<Toaster />);
    const region = document.querySelector<HTMLElement>("[aria-live=polite]")!;
    expect(region.className).toContain("z-40");
    expect(region.className).not.toMatch(/z-\[?[5-9]\d/);
  });
});

describe("CopyValue", () => {
  it("copies the full text while showing its label", async () => {
    installTauri();
    const writeText = vi.fn(async () => {});
    const user = userEvent.setup();
    clipboard(writeText);
    render(<CopyValue text="http://127.0.0.1:56235" label=":56235" />);
    await user.click(screen.getByRole("button", { name: /:56235/ }));
    expect(writeText).toHaveBeenCalledWith("http://127.0.0.1:56235");
  });

  it("says when the clipboard refuses", async () => {
    installTauri();
    const user = userEvent.setup();
    clipboard(async () => {
      throw new Error("no permission");
    });
    vi.spyOn(document, "hasFocus").mockReturnValue(true);
    vi.spyOn(console, "warn").mockImplementation(() => {});
    render(
      <>
        <CopyValue text="secret" />
        <Toaster />
      </>,
    );
    await user.click(screen.getByRole("button", { name: /secret/ }));
    await flush();
    expect(document.body.textContent).toContain("no permission");
  });
});

describe("RadioRow", () => {
  it("makes the group one Tab stop, the arrow keys moving (and picking) between enabled rows", async () => {
    const user = userEvent.setup();
    const Group = () => {
      const [value, setValue] = useState<string | null>(null);
      const row = (id: string, disabled = false) => (
        <RadioRow key={id} selected={value === id} onSelect={() => setValue(id)} title={id} subtitle={`${id} subtitle`} disabled={disabled} />
      );
      return (
        <>
          <button type="button">before</button>
          <RadioList label="Where">{[row("a"), row("b"), row("off", true), row("c")]}</RadioList>
          <button type="button">after</button>
        </>
      );
    };
    render(<Group />);
    const radio = (name: string) => screen.getByRole("radio", { name: new RegExp(`^${name}`) });
    const stops = () => screen.getAllByRole("radio").filter((r) => r.tabIndex === 0);
    // Nothing picked yet: the first enabled row is the only stop.
    expect(stops()).toEqual([radio("a")]);
    screen.getByRole("button", { name: "before" }).focus();
    await user.tab();
    expect(document.activeElement).toBe(radio("a"));
    await user.tab();
    expect(document.activeElement).toBe(screen.getByRole("button", { name: "after" }));
    // Arrows move and pick, skipping the disabled row, wrapping at the ends.
    radio("a").focus();
    await user.keyboard("{ArrowDown}");
    expect(document.activeElement).toBe(radio("b"));
    expect(radio("b").getAttribute("aria-checked")).toBe("true");
    await user.keyboard("{ArrowRight}");
    expect(document.activeElement).toBe(radio("c"));
    await user.keyboard("{ArrowDown}");
    expect(document.activeElement).toBe(radio("a"));
    await user.keyboard("{ArrowUp}{ArrowLeft}");
    expect(document.activeElement).toBe(radio("b"));
    await user.keyboard("{End}");
    expect(document.activeElement).toBe(radio("c"));
    await user.keyboard("{Home}");
    expect(document.activeElement).toBe(radio("a"));
    expect(stops()).toEqual([radio("a")]);
    // Other keys are left alone; the picked row stays the stop after a click elsewhere.
    await user.keyboard("x");
    await user.click(radio("c"));
    expect(stops()).toEqual([radio("c")]);
    // Long descriptions get two lines, and the whole text in a tooltip.
    const sub = screen.getByText("c subtitle");
    expect(sub.className).toContain("line-clamp-2");
    expect(sub.getAttribute("title")).toBe("c subtitle");
  });

  it("picks with a click, Enter or Space, and never when disabled", async () => {
    const pick = vi.fn();
    const blocked = vi.fn();
    const user = userEvent.setup();
    render(
      <RadioList label="Where">
        <RadioRow selected onSelect={pick} title="Here" subtitle="this machine" leading={<i>L</i>} trailing={<i>T</i>} />
        <RadioRow selected={false} onSelect={blocked} title="There" subtitle="a server" disabled hint="No server yet" compact />
      </RadioList>,
    );
    const [here, there] = screen.getAllByRole("radio");
    expect(here.getAttribute("aria-checked")).toBe("true");
    expect(there.getAttribute("aria-disabled")).toBe("true");
    expect(there.getAttribute("title")).toBe("No server yet");
    await user.click(here);
    here.focus();
    await user.keyboard("{Enter} ");
    await user.keyboard("a");
    expect(pick).toHaveBeenCalledTimes(3);
    await user.click(there);
    fireEvent.keyDown(there, { key: "Enter" });
    expect(blocked).not.toHaveBeenCalled();
  });

  it("ignores keys pressed inside its children", () => {
    const pick = vi.fn();
    render(
      <RadioRow selected={false} onSelect={pick} title="t" subtitle="s">
        <input aria-label="inner" />
      </RadioRow>,
    );
    fireEvent.keyDown(screen.getByLabelText("inner"), { key: "Enter" });
    expect(pick).not.toHaveBeenCalled();
  });
});

describe("LogConsole", () => {
  it("renders nothing without lines", () => {
    const { container } = render(<LogConsole lines={[]} running />);
    expect(container.innerHTML).toBe("");
  });

  it("times a run, then folds away when it succeeds, and unfolds for the next run", async () => {
    vi.useFakeTimers();
    const { rerender } = render(<LogConsole lines={["\n\nPulling", "", "", "", "Started"]} running title="Deploy" collapseOnDone />);
    act(() => vi.advanceTimersByTime(1300));
    expect(document.body.textContent).toContain("Deploy");
    expect(document.querySelector("pre")?.textContent).toBe("Pulling\n\nStarted");
    rerender(<LogConsole lines={["Pulling", "Started"]} running={false} title="Deploy" collapseOnDone />);
    expect(document.querySelector("pre")).toBeNull();
    rerender(<LogConsole lines={["Pulling again"]} running title="Deploy" collapseOnDone />);
    expect(document.querySelector("pre")).not.toBeNull();
  });

  it("stays open on a failure, and folds by hand", () => {
    const { rerender } = render(<LogConsole lines={["Pulling"]} running collapseOnDone />);
    rerender(<LogConsole lines={["Pulling", "✗ Port taken"]} running={false} collapseOnDone />);
    expect(document.querySelector("pre")?.textContent).toContain("Port taken");
    expect(document.body.textContent).toMatch(/failed/i);
    fireEvent.click(screen.getByRole("button"));
    expect(document.querySelector("pre")).toBeNull();
    fireEvent.click(screen.getByRole("button"));
    expect(document.querySelector("pre")).not.toBeNull();
  });
});

describe("small pieces", () => {
  it("Meter turns warm over 75% and fails over 90%, clamped", () => {
    const fill = (v: number) => {
      const { container, unmount } = render(<Meter value={v} />);
      const el = container.firstElementChild!.firstElementChild as HTMLElement;
      const out = [el.className.match(/meter-fill(-\w+)?/)![0], el.style.width];
      unmount();
      return out;
    };
    expect(fill(-5)).toEqual(["meter-fill", "0%"]);
    expect(fill(80)).toEqual(["meter-fill-warn", "80%"]);
    expect(fill(150)).toEqual(["meter-fill-fail", "100%"]);
  });

  it("Sparkline draws a flat line from one value, or none", () => {
    const { container, rerender } = render(<Sparkline values={[50]} />);
    expect(container.querySelectorAll("polyline")[1].getAttribute("points")).toBe("0.0,18.0 160.0,18.0");
    rerender(<Sparkline values={[]} warn />);
    expect(container.querySelectorAll("polyline")[1].getAttribute("style")).toContain("--warning");
  });

  it("Tip wraps its control only when it has text", () => {
    const { rerender } = render(
      <Tip>
        <button>go</button>
      </Tip>,
    );
    expect(screen.queryByRole("tooltip")).toBeNull();
    rerender(
      <Tip text="Shut down the lab" align="start">
        <button>go</button>
      </Tip>,
    );
    expect(screen.getByRole("tooltip").className).toContain("left-0");
  });

  it("NoticePanel shows its message", () => {
    render(<NoticePanel tone="fail">Docker is not running</NoticePanel>);
    expect(screen.getByText("Docker is not running")).toBeTruthy();
  });
});

describe("ErrorScreen", () => {
  it("names an error without a message, reloads, and says when copying fails", async () => {
    installTauri();
    vi.spyOn(console, "error").mockImplementation(() => {});
    vi.spyOn(console, "warn").mockImplementation(() => {});
    vi.spyOn(document, "hasFocus").mockReturnValue(true);
    const user = userEvent.setup();
    clipboard(async () => {
      throw new Error("clipboard locked");
    });
    const reload = vi.fn();
    vi.spyOn(window, "location", "get").mockReturnValue({ ...window.location, reload, pathname: "/", search: "" });
    const err = new Error("");
    err.stack = undefined;
    render(
      <>
        <ErrorScreen error={err} />
        <Toaster />
      </>,
    );
    expect(screen.getByRole("alert").textContent).toBe("Error");
    // No retry without a handler.
    expect(screen.queryByRole("button", { name: /try again/i })).toBeNull();
    await user.click(screen.getByRole("button", { name: /reload/i }));
    expect(reload).toHaveBeenCalled();
    await user.click(screen.getByRole("button", { name: /copy details/i }));
    await flush();
    expect(document.body.textContent).toContain("clipboard locked");
  });
});

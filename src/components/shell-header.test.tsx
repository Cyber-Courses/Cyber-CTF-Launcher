// @vitest-environment jsdom
import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { ShellHeader } from "@/components/shell/shell-header";
import { readyReport, stoppedDockerReport } from "@/test/fixtures";

// The header's engine line when the report says Docker isn't running: the words, not a guess.

describe("ShellHeader", () => {
  it("says Docker not running when the report says the engine is stopped", () => {
    render(<ShellHeader crumbs={["Overview"]} report={{ ...stoppedDockerReport(), vmProviders: [] }} checkError={null} />);
    expect(screen.getByText("Docker not running")).toBeTruthy();
    expect(document.body.textContent).not.toContain("Docker Desktop");
  });

  it("says Docker not running even when the report names an installed engine that is stopped", () => {
    render(
      <ShellHeader crumbs={["Overview"]} report={{ ...readyReport(), dockerRunning: false, dockerEngine: null, dockerEnginesRunning: [] }} checkError={null} />,
    );
    expect(screen.getByText("Docker not running")).toBeTruthy();
  });

  it("names the running engine otherwise", () => {
    render(<ShellHeader crumbs={["Overview"]} report={readyReport()} checkError={null} />);
    expect(screen.queryByText("Docker not running")).toBeNull();
  });
});

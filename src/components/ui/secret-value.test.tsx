// @vitest-environment jsdom
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it } from "vitest";
import { SecretValue } from "@/components/ui/secret-value";

const labels = { show: "Show key", hide: "Hide key", copy: "Copy key" };

describe("SecretValue", () => {
  it("is masked until Show, its copy named without it, and copies it masked or not", async () => {
    const user = userEvent.setup();
    render(<SecretValue text="s3cret-value" labels={labels} />);
    expect(screen.getByText("••••••••")).toBeTruthy();
    expect(document.body.innerHTML).not.toContain("s3cret-value");
    await user.click(screen.getByRole("button", { name: "Copy key" }));
    expect(await navigator.clipboard.readText()).toBe("s3cret-value");
    await user.click(screen.getByRole("button", { name: "Show key" }));
    expect(screen.getByText("s3cret-value")).toBeTruthy();
    await user.click(screen.getByRole("button", { name: "Hide key" }));
    expect(screen.queryByText("s3cret-value")).toBeNull();
  });
});

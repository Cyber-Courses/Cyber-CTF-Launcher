// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import { SettingsWindowView } from "@/components/shell/settings-window";
import { confirmDestructive } from "@/lib/confirm";

const question = { title: "Remove VirtualBox?", message: "The labs that use it stop working.", confirm: "Remove" };

afterEach(cleanup);

describe("the Settings window (#383)", () => {
  it("asks its own questions in the app's dialog: no, then yes, then a later one still asks", async () => {
    // Regression: its root mounted no dialog. A clean-up or a tool's removal waited for one that
    // never opened, and every confirmation after it in the window answered no.
    render(<SettingsWindowView auth={null} onAuthChange={() => {}} />);
    const ask = async (button: string) => {
      const answer = confirmDestructive(question);
      const dialog = await screen.findByRole("alertdialog");
      expect(within(dialog).getByText("Remove VirtualBox?")).toBeTruthy();
      fireEvent.click(within(dialog).getByRole("button", { name: button }));
      return answer;
    };
    expect(await ask("Cancel")).toBe(false);
    expect(await ask("Remove")).toBe(true);
    expect(await ask("Remove")).toBe(true);
    expect(screen.queryByRole("alertdialog")).toBeNull();
  });
});

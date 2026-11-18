import { screen, within } from "@testing-library/react";
import { expect } from "vitest";
import type userEvent from "@testing-library/user-event";

/** Goes on with an install, in the dialog that says what it asks of the player before it starts:
 *  Install. The dialog only informs: there is nothing to tick. */
export async function agreeToInstall(user: ReturnType<typeof userEvent.setup>) {
  const dialog = await screen.findByRole("dialog", { name: /^before .+ installs$/i });
  expect(within(dialog).queryAllByRole("checkbox")).toEqual([]);
  await user.click(within(dialog).getByRole("button", { name: /^install$/i }));
}

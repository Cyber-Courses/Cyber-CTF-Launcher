import { screen } from "@testing-library/react";
import type { UserEvent } from "@testing-library/user-event";
import { expect } from "vitest";

/** The attack box's `password` on the page: masked (nowhere in it, its copy button's tooltip
 *  included), copied while masked, shown on Show and hidden again on Hide. */
export async function expectMaskedPassword(user: UserEvent, password: string) {
  expect(document.body.innerHTML).not.toContain(password);
  await user.click(screen.getByRole("button", { name: "Copy password" }));
  expect(await navigator.clipboard.readText()).toBe(password);
  await user.click(screen.getByRole("button", { name: "Show password" }));
  expect(screen.getByText(password)).toBeTruthy();
  await user.click(screen.getByRole("button", { name: "Hide password" }));
  expect(screen.queryByText(password)).toBeNull();
}

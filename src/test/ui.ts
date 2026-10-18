import { act, screen } from "@testing-library/react";
import type { UserEvent } from "@testing-library/user-event";

/** Lets pending promises (mocked Tauri invokes) and the renders they cause settle. */
export const flush = (ms = 20) =>
  act(async () => {
    await new Promise((r) => setTimeout(r, ms));
  });

/** Runs `fn` (resolving a pending read, say) and lets what it settles land, with the renders it
 *  causes. */
export const settle = (fn: () => void) =>
  act(() => {
    fn();
    return Promise.resolve();
  });

/** The first button whose text or accessible name matches `name`. */
export function button(name: RegExp): HTMLElement {
  // A menu's items are buttons too, to click.
  const all = () => [...screen.queryAllByRole("button"), ...screen.queryAllByRole("menuitem")];
  const found = all().find((b) => name.test(b.textContent) || name.test(b.getAttribute("aria-label") ?? ""));
  if (!found)
    throw new Error(
      `no button matching ${String(name)}: ${all()
        .map((b) => b.textContent)
        .join(" | ")}`,
    );
  return found;
}

/** Types `value` into every empty text field on screen. */
export async function fillEmptyFields(user: UserEvent, value = "10.0.0.5") {
  for (const el of Array.from(
    document.body.querySelectorAll<HTMLInputElement>("input:not([type]), input[type=text], input[type=password], input[type=number], textarea"),
  )) {
    if (el.disabled || el.readOnly || el.value) continue;
    await user.type(el, el.type === "number" ? "8" : value);
  }
}

const CONTROLS = "button, [role=radio], [role=switch], [role=checkbox], [role=tab], [role=menuitem], [role=option], input[type=checkbox], input[type=radio]";

/**
 * Clicks every enabled control (buttons, radios, switches, checkboxes) on screen once, in
 * document order, skipping those whose text matches `skip`, re-reading the screen after each
 * click since a click can open or close parts of it. Exercises a panel's handlers without a
 * scripted path for each; assertions on the outcomes belong in the tests that call it.
 */
export async function clickEverything(user: UserEvent, { skip, max = 40 }: { skip?: RegExp; max?: number } = {}) {
  const seen = new Set<string>();
  for (let i = 0; i < max; i++) {
    const next = Array.from(document.body.querySelectorAll<HTMLElement>(CONTROLS)).find((el) => {
      const key = `${el.getAttribute("role") ?? el.tagName}:${el.textContent}:${el.getAttribute("aria-label") ?? ""}`;
      if (seen.has(key)) return false;
      if (el.hasAttribute("disabled") || el.getAttribute("aria-disabled") === "true") return false;
      if (skip && (skip.test(el.textContent.trim()) || skip.test(el.getAttribute("aria-label") ?? ""))) return false;
      return true;
    });
    if (!next) return;
    seen.add(`${next.getAttribute("role") ?? next.tagName}:${next.textContent}:${next.getAttribute("aria-label") ?? ""}`);
    await user.click(next);
    await flush(5);
  }
}

// Helpers the Settings tests share. Not part of the app: only tests import them.

import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { vi } from "vitest";
import { AttackBoxRows } from "@/features/settings/attack-box-rows";
import { InstalledToolsPanel } from "@/features/settings/installed-tools-panel";
import { SharedFolderControl } from "@/features/settings/shared-folder";
import { installTauri, rejectWith, type Handler } from "@/test/tauri";
import { flush } from "@/test/ui";

/** The attack box rows, once read, with the "Custom image" choice opened: its field, its Save
 *  button and the `onSaved` spy. */
export async function openCustomImage() {
  installTauri();
  const onSaved = vi.fn();
  const user = userEvent.setup();
  render(<AttackBoxRows onSaved={onSaved} />);
  await flush();
  await user.click(screen.getByRole("radio", { name: /custom image/i }));
  return {
    user,
    onSaved,
    field: () => screen.getByPlaceholderText("registry/image:tag"),
    save: () => screen.getAllByRole("button", { name: /^save$/i })[0],
  };
}

/** The installed tools panel over the commands `handlers` answer, once read, and how to ask to
 *  remove its first tool. */
export async function renderInstalledTools(handlers: Partial<Record<string, Handler>>) {
  const calls = installTauri(handlers);
  const user = userEvent.setup();
  render(<InstalledToolsPanel />);
  await flush();
  const askToRemove = () => user.click(screen.getAllByRole("button", { name: /remove/i })[0]);
  return { calls, user, askToRemove };
}

/** The shared folder control whose saves fail (as the core does) until `save.fail` is turned off,
 *  once read, with its switch pressed once; `toggle` presses it again. */
export async function toggleFailingSharedFolder() {
  const save = { fail: true };
  installTauri({
    shared_folder_set: (a) => {
      if (save.fail) rejectWith("permission denied");
      return { enabled: !!a?.enabled, path: typeof a?.path === "string" ? a.path : "", mountPoint: "/shared" };
    },
  });
  const user = userEvent.setup();
  render(<SharedFolderControl />);
  await flush();
  const toggle = async () => {
    await user.click(screen.getByRole("switch"));
    await flush();
  };
  await toggle();
  return { save, toggle };
}

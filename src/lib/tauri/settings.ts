import { invoke } from "@tauri-apps/api/core";
import type { Change, Imported, PortMode, Settings, TestRecord, TestResult, VmForm } from "@/lib/tauri/generated";

export type { Change, Imported, PortMode, Settings, TestRecord, TestResult, VmForm };

/** Every setting, as the core keeps them (`crate::settings`). */
export const settingsGet = () => invoke<Settings>("settings_get");

/** Applies `changes`, all or none; answers the settings after them (every window is told). */
export const settingsChange = (changes: Change[]) => invoke<Settings>("settings_change", { changes });

/** Hands the settings an earlier version kept in this webview's storage (`items`, by key) to the
 *  core; answers the keys it took, to remove here, and the settings after. */
export const settingsImportWebview = (items: Record<string, string>) => invoke<Imported>("settings_import_webview", { items });

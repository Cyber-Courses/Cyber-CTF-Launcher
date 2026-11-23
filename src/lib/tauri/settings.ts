import { invoke } from "@tauri-apps/api/core";
import type { Change, PortMode, Settings, TestRecord, TestResult, VmForm } from "@/lib/tauri/generated";

export type { Change, PortMode, Settings, TestRecord, TestResult, VmForm };

/** Every setting, as the core keeps them (`crate::settings`). */
export const settingsGet = () => invoke<Settings>("settings_get");

/** Applies `changes`, all or none; answers the settings after them (every window is told). */
export const settingsChange = (changes: Change[]) => invoke<Settings>("settings_change", { changes });

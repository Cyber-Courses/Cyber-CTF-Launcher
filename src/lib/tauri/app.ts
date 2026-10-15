import { invoke } from "@tauri-apps/api/core";
import { invokeUnit } from "@/lib/tauri/invoke";
import type { AgentInfo } from "@/lib/tauri/generated";

export type { AgentInfo };

/** Opens Settings in its own window (reused/focused if already open). */
export const openSettings = () => invokeUnit("open_settings");
/** Relaunches the app (applies an installed update). */
export const restartApp = () => invokeUnit("restart_app");
/** Opens the folder of the app's log files in the system's file manager. */
export const openLogFolder = () => invokeUnit("open_log_folder");

/** This machine's launcher-agent identity (for the Settings screen). */
export const agentInfo = () => invoke<AgentInfo>("agent_info");

/** Answer a cloud-launch confirmation the agent asked for (a website launch onto a cloud account). */
export const confirmLaunch = (sessionId: string, approve: boolean) => invokeUnit("confirm_launch", { sessionId, approve });

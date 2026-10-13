import { invoke } from "@tauri-apps/api/core";
import type { AgentInfo } from "@/lib/tauri/generated";

export type { AgentInfo };

/** Opens Settings in its own window (reused/focused if already open). */
export const openSettings = () => invoke<void>("open_settings");
/** Relaunches the app (applies an installed update). */
export const restartApp = () => invoke<void>("restart_app");

/** This machine's launcher-agent identity (for the Settings screen). */
export const agentInfo = () => invoke<AgentInfo>("agent_info");

/** Answer a cloud-launch confirmation the agent asked for (a website launch onto a cloud account). */
export const confirmLaunch = (sessionId: string, approve: boolean) => invoke<void>("confirm_launch", { sessionId, approve });

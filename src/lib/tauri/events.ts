import { DROP, channel } from "@/lib/event-channel";
import { EVENTS } from "@/lib/tauri/generated/Events";
import { PROVIDER_NAMES } from "@/lib/tauri/generated/ProviderNames";
import type { Provider } from "@/lib/tauri/generated/Provider";

// The events the core emits to the webview (Rust's `app::events`), each with its payload's
// decoder. The launch prompt's is beside its only reader (`features/app/launch-confirm.tsx`).

/** `value` when it is a provider's id, else null. */
export function providerId(value: unknown): Provider | null {
  return typeof value === "string" && value in PROVIDER_NAMES ? (value as Provider) : null;
}

/** Quitting waits on labs in flight: how many (one when the core doesn't say). */
export const quitBlocked = channel<number>(EVENTS.quitBlocked, (n) => (typeof n === "number" ? n : 1));

/** The guided machine setup is asked to show a step (and the hypervisor it is for). */
export type SetupStepAsked = { step: string; provider: Provider | null };

/** A "Fix" asked the open machine-setup window to show a step. */
export const machineSetupStep = channel<SetupStepAsked>(EVENTS.machineSetupStep, (at) => {
  if (typeof at !== "object" || at === null || !("step" in at) || typeof at.step !== "string") return DROP;
  return { step: at.step, provider: "provider" in at ? providerId(at.provider) : null };
});

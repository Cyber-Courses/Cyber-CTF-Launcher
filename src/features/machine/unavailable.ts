import type { Unavailable } from "@/lib/tauri";
import { translate, type MessageKey, type Vars } from "@/lib/i18n";

/** `t` from `useT()` in a component, `translate` elsewhere. */
type Translate = (key: MessageKey, vars?: Vars) => string;

/** Why a hypervisor can't run labs here, in the player's language: the core's code for it
 *  (`ProviderStatus.reason`, a `blocked` setup step), with the values its sentence needs. */
export function unavailableText(reason: Unavailable, t: Translate = translate): string {
  if (reason.code === "notInstalled" && reason.args.program === null) return t("machine.unavailable.notInstalledNoProgram");
  return t(`machine.unavailable.${reason.code}`, "args" in reason ? reason.args : undefined);
}

import { attackBoxAttach, type Runtime } from "@/lib/tauri";
import { startOnce } from "@/lib/start-once";
import { savedAttackImage } from "@/lib/settings";

/** Adds the attack box to lab `labId` (run as `runtime`) onto `network` (null: the seat the lab
 *  declares, else its first network), from the attack box image the learner saved in Settings (a
 *  container box runs it; none: the backend's default; a VM box is Kali), or joins the add already
 *  under way: a second one would race the first on the same run. The lab page and the auto-start
 *  with each lab share it. */
export const attachAttackBox = (labId: string, runtime: Runtime, network: string | null, onLog: (line: string) => void = () => {}) =>
  startOnce(`attach:${labId}`, (log) => attackBoxAttach(labId, runtime, network, savedAttackImage(), log), onLog);

import { translate } from "@/lib/i18n";

// A tool's live progress line, as the player reads it. The OVF Tool, which uploads each VM of a
// lab on ESXi, redraws `Disk progress: 8%` in place (the runner hands each redraw on as a line).

/** How far an upload is, in percent, when `line` is the OVF Tool's progress (`Disk progress: 8%`). */
export function uploadPercent(line: string): number | null {
  const m = /\bDisk progress: (\d{1,3})%/.exec(line);
  return m ? Math.min(100, Number(m[1])) : null;
}

/** `line` as a step's live detail: an upload's progress as "Uploading: 8%", any other as it is. */
export function progressText(line: string): string {
  const percent = uploadPercent(line);
  return percent === null ? line : translate("common.progress.uploading", { percent });
}

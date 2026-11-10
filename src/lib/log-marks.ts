import { DONE_MARK, FAILED_MARK } from "@/lib/tauri";

// The marks a run's closing log line starts with, from the backend (`deploy_log`): a failure's
// (`✗ <why>`) and a run's that went through (`✓ Lab is running`, the translations' own lines).

/** A log's closing line for a failure: `✗ <why>`. */
export const failedLine = (why: string) => `${FAILED_MARK} ${why}`;

/** Whether `line` is a failure's closing line. */
export const isFailedLine = (line: string) => line.startsWith(FAILED_MARK);

/** Whether `line` is the closing line of a run that went through. */
export const isDoneLine = (line: string) => line.startsWith(DONE_MARK);

/** A failure's closing line without its mark: why it failed. */
export const failureText = (line: string) => line.slice(FAILED_MARK.length).trimStart();

/** The failure a log ended on (its last failed line, without the mark), or null. */
export function failureOf(lines: readonly string[]): string | null {
  const line = lines.findLast(isFailedLine);
  return line === undefined ? null : failureText(line);
}

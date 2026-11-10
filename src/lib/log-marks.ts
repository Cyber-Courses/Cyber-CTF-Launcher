import { DONE_MARK, FAILED_CODE, FAILED_MARK } from "@/lib/tauri";
import type { ErrorCode } from "@/lib/errors";

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

/** A failed line as the backend writes it in a worker's log, its error's code right after the
 *  mark (`✗[portInUse] Host port 8000…`, `FAILED_CODE`): the line as shown (`✗ <why>`) and the
 *  code, for a page that reads the run back without its live error. Any other line as it is. */
export function readFailedLine(line: string): { line: string; code: ErrorCode | null } {
  const [open, close] = FAILED_CODE;
  const end = line.indexOf(close);
  if (!line.startsWith(FAILED_MARK + open) || end < 0) return { line, code: null };
  return { line: failedLine(line.slice(end + close.length).trimStart()), code: line.slice(FAILED_MARK.length + open.length, end) as ErrorCode };
}

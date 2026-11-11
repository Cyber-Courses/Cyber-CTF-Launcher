import type { MessageKey } from "@/lib/i18n";
import type { ErrorCode } from "@/lib/errors";
import type { LabOperation } from "@/features/labs/runs/operations";

// What a failed lab operation's error means, for a hint on what to do about it. The launcher's own
// failures are known by their code (src-tauri error.rs `Code`): the error this window saw, or the
// one a worker's log ends on (`✗[code] …`, read back after a reload). What the tools themselves
// print (Docker's bind error, a full disk, a download that failed) has no code: it is recognised
// by their words.

export type FailureKind = "ports" | "docker" | "dockerAccess" | "signedOut" | "network" | "disk" | "tool";

const BY_CODE: Partial<Record<ErrorCode, FailureKind>> = {
  portInUse: "ports",
  dockerNotRunning: "docker",
  dockerDenied: "dockerAccess",
  dockerDeniedWindows: "dockerAccess",
  signedOut: "signedOut",
  sessionExpired: "signedOut",
  toolMissing: "tool",
};

// The tools' own words, most specific first: the daemon's "permission denied" before its "cannot
// connect".
const RULES: [FailureKind, RegExp][] = [
  // Docker's bind error when it races the launcher's own port check.
  ["ports", /port is already allocated|address already in use|bind for \S+ failed/i],
  ["dockerAccess", /permission denied while trying to connect to the docker daemon/i],
  ["docker", /cannot connect to the docker daemon|is the docker daemon running/i],
  ["disk", /no space left on device/i],
  ["network", /error sending request|could not resolve host|couldn't download|network is unreachable|connection refused|tls handshake/i],
];

/** What kind of failure this is, when it is one with a known fix: by the error's `code` when
 *  there is one, else by the words of `message`. */
export function failureKind(message: string | null | undefined, code?: ErrorCode | null): FailureKind | null {
  const known = code ? BY_CODE[code] : undefined;
  if (known) return known;
  if (!message) return null;
  return RULES.find(([, re]) => re.test(message))?.[0] ?? null;
}

/** The hint for a kind of failure (labs.failure.hint.*). */
export const failureHintKey = (kind: FailureKind): MessageKey => `labs.failure.hint.${kind}`;

/** The title of a failed operation's notice and toast ("Couldn't start Blind orders"): every
 *  operation has one (labs.failure.title.*). */
export const failureTitleKey = (op: LabOperation): MessageKey => `labs.failure.title.${op}`;

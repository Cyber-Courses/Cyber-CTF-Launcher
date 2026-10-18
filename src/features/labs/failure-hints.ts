import type { MessageKey } from "@/lib/i18n";
import type { ErrorCode } from "@/lib/errors";

// What a failed lab operation's error means, for a hint on what to do about it. An error this
// window saw is known by its code (src-tauri error.rs `Code`). A failure read back from a
// worker's log file (a deploy that ran through a reload) is only text, in English, and so is
// what the tools themselves print (Docker's bind error, a full disk): those are recognised by
// their words.

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

// Most specific first: Docker's "can't use it yet" message also says "Docker is running".
const RULES: [FailureKind, RegExp][] = [
  // runtime/docker/lifecycle.rs's own check, and Docker's bind error when it races it.
  ["ports", /already in use on this machine|port is already allocated|address already in use|bind for \S+ failed/i],
  // exec/docker.rs docker_denied_message, and the daemon's own answer.
  ["dockerAccess", /can't use it yet|permission denied while trying to connect to the docker daemon/i],
  ["docker", /docker isn't running|cannot connect to the docker daemon|is the docker daemon running/i],
  // account/auth.rs: the session is gone.
  ["signedOut", /\bsigned out\b|session expired/i],
  ["disk", /no space left on device/i],
  // error.rs ToolMissing, and the "X isn't installed" ones (Terraform, QEMU's emulator).
  ["tool", /is not installed or not on path|isn't installed/i],
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

/** The operations whose failure is reported by name (labs.failure.title.*). */
const TITLED = ["launch", "resume", "pause", "shutdown", "stop", "provision"] as const;

/** The title of a failed operation's notice and toast ("Couldn't start Blind orders"). */
export function failureTitleKey(op: string): MessageKey | null {
  const known = TITLED.find((o) => o === op);
  return known ? `labs.failure.title.${known}` : null;
}

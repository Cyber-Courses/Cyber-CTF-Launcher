import { describe, expect, it } from "vitest";
import { failureHintKey, failureKind, failureTitleKey } from "@/features/labs/failure-hints";
import { runFailure, startFailed } from "@/features/labs/runs/deploy-runs";

describe("failureKind", () => {
  it("recognises the tools' own words that have a known fix", () => {
    const cases: [string, ReturnType<typeof failureKind>][] = [
      ["Error response from daemon: Bind for 0.0.0.0:8080 failed: port is already allocated", "ports"],
      ["listen tcp 0.0.0.0:8443: bind: address already in use", "ports"],
      ["permission denied while trying to connect to the Docker daemon socket at unix:///var/run/docker.sock", "dockerAccess"],
      ["Cannot connect to the Docker daemon at unix:///var/run/docker.sock. Is the docker daemon running?", "docker"],
      ["write /var/lib/docker/tmp: no space left on device", "disk"],
      ["error sending request for url (https://api.cyberctf.org/graphql)", "network"],
      ["Couldn't download the Terraform provider plugins (the first run needs internet).", "network"],
      ["curl: (6) Could not resolve host: download.docker.com", "network"],
      ["dial tcp 10.0.0.5:8006: connect: network is unreachable", "network"],
      ["dial tcp 127.0.0.1:2375: connect: connection refused", "network"],
      ["remote error: tls handshake failure", "network"],
      ["This lab is already running on this machine. Stop it before starting it again.", null],
    ];
    for (const [message, kind] of cases) expect(failureKind(message), message).toBe(kind);
    expect(failureKind(null)).toBeNull();
    expect(failureHintKey("ports")).toBe("labs.failure.hint.ports");
  });

  it("knows the launcher's own errors by their code alone, not by their English words", () => {
    // Regression: the hints matched the core's sentences, so a rewording there dropped them.
    expect(failureKind("Host port 8080 is already in use on this machine (another lab or app holds it).")).toBeNull();
    expect(failureKind("Docker isn't running. Start your container engine.")).toBeNull();
    expect(failureKind("Host port 8080 is reworded", "portInUse")).toBe("ports");
  });

  it("knows an error by its code, whatever language its message is in", () => {
    expect(failureKind("Le port 8080 de l'hôte est déjà utilisé sur cette machine", "portInUse")).toBe("ports");
    expect(failureKind("Docker が実行されていません。", "dockerNotRunning")).toBe("docker");
    expect(failureKind("Docker läuft, aber dein Konto kann es noch nicht nutzen.", "dockerDenied")).toBe("dockerAccess");
    expect(failureKind("Sua sessão expirou", "sessionExpired")).toBe("signedOut");
    expect(failureKind("terraform n'est pas installé", "toolMissing")).toBe("tool");
    // A code with no hint of its own still lets the words of a tool's output speak.
    expect(failureKind("write /tmp: no space left on device", "other")).toBe("disk");
    expect(failureKind("Une autre opération est déjà en cours", "alreadyInProgress")).toBeNull();
  });

  it("names the operations it reports", () => {
    expect(failureTitleKey("stop")).toBe("labs.failure.title.stop");
    expect(failureTitleKey("reboot")).toBeNull();
  });
});

describe("a run's failure", () => {
  const run = (op: string, logs: string[], busy = false) => ({ busy, logs, times: logs.map(() => 0), op });
  it("is its last ✗ line once it is over", () => {
    expect(runFailure(run("launch", ["==> pulling", "✗ boom"]))).toBe("boom");
    expect(runFailure(run("launch", ["✗ boom"], true))).toBeNull();
    expect(runFailure(run("launch", ["✓ Lab is running"]))).toBeNull();
    expect(runFailure(undefined)).toBeNull();
  });
  it("is a failed start only for a launch", () => {
    expect(startFailed(run("launch", ["✗ boom"]))).toBe(true);
    expect(startFailed(run("stop", ["✗ boom"]))).toBe(false);
    expect(startFailed(undefined)).toBe(false);
  });
});

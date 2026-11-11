// The leftover audit's inventory: a snapshot's probes (tests/audit/snapshot.sh, snapshot.ps1,
// snapshot-server.sh) read into items, one parser a kind. Each parser takes its tool's own output
// as the snapshot recorded it; inventory.test.ts holds each to a recorded output.

import { HOST_PARSERS } from "./parse-host.ts";
import { MACHINE_PARSERS } from "./parse-machines.ts";

/** One thing on the machine: its kind, the key that tells it from the others of its kind (a
 *  Docker id, a path, a VM's UUID), the name a report shows, the names that tie it to a run (its
 *  Compose project, its lab), when it was made (epoch seconds) when its tool says, the paths it
 *  lives at or was made from, and its process when it has one. */
export type Item = {
  kind: string;
  key: string;
  name: string;
  refs: string[];
  created?: number;
  paths: string[];
  pid?: number;
};

/** A probe as a snapshot records it: the tool's output, or why there is none. */
export type Probe = { kind: string; tool: string; status: string; note: string; output: string };

/** A snapshot: the machine, when it was taken, the run's roots it was given and its probes. */
export type Snapshot = { audit: number; os: string; host: string; user: string; taken: number; roots: string[]; probes: Probe[] };

/** A parser: the items of a probe's output. `taken` is the snapshot's time (a process's start is
 *  told from its age). */
export type Parser = (output: string, taken: number) => Item[];

/** Every kind's parser. */
export const PARSERS: Record<string, Parser> = { ...HOST_PARSERS, ...MACHINE_PARSERS };

/** The kind's items, or an error naming the kind with no parser. */
export function parseProbe(probe: Probe, taken: number): Item[] {
  const parser = PARSERS[probe.kind] as Parser | undefined;
  if (!parser) throw new Error(`no parser for the kind ${probe.kind} (tests/audit/inventory.ts)`);
  return parser(probe.output, taken);
}

/** A snapshot read from its JSON text, checked for the fields the diff needs. */
export function readSnapshot(text: string, source: string): Snapshot {
  const data = JSON.parse(text.replace(/^﻿/, "")) as Partial<Snapshot>;
  if (data.audit !== 1 || !Array.isArray(data.probes) || typeof data.taken !== "number") {
    throw new Error(`${source} isn't a leftover audit snapshot (tests/audit/snapshot.sh writes them)`);
  }
  return { audit: 1, os: data.os ?? "", host: data.host ?? "", user: data.user ?? "", taken: data.taken, roots: data.roots ?? [], probes: data.probes };
}

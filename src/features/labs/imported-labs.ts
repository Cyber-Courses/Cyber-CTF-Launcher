"use client";

import { useEffect, useSyncExternalStore } from "react";
import { importedLabs, type ImportedLab } from "@/lib/tauri";
import type { Lab, LabRuntimeInfo } from "@/features/labs/use-labs";
import { refreshStatus, useLabState } from "@/features/labs/lab-store";
import { useDeployingLabs } from "@/features/labs/runs/operations";
import { structurallyEqual } from "@/lib/equal";
import { warn } from "@/lib/failure";
import { registerReset } from "@/lib/module-state";

// The labs imported from a folder (Labs > Import a lab) installed here. No catalogue lists them:
// what their page shows comes from their own `.ctf/` folder, read by the core from the installed
// copy (`imported_labs`), and they run as containers on this machine only.

/** How an imported lab's id starts (`local-<name>`, the core's `labs::imported::PREFIX`). */
const PREFIX = "local-";

/** Whether `id` is an imported lab's. */
export const isImportedId = (id: string) => id.startsWith(PREFIX);

/** Where an imported lab runs: as containers, here (the import starts it so, and nothing else). */
const RUNTIME: LabRuntimeInfo = { runtime: "DOCKER", architectures: [], providers: [], docker: true, vm: false, cloud: false };

/** An imported lab as the pages read a lab: its id for a slug, its `.ctf/metadata.json`'s title
 *  (else its spec's name), description, category, difficulty and capabilities when it gives them,
 *  none of them when it doesn't. No question: its challenges are its `.ctf/objectives.json`'s. */
export function importedLab(i: ImportedLab): Lab {
  const m = i.metadata;
  return {
    id: i.id,
    slug: i.id,
    title: m?.title ?? i.name,
    description: m?.description ?? null,
    question: null,
    difficulty: m?.difficulty ?? 0,
    category: m?.category ?? "",
    runtime: RUNTIME,
    skills: (m?.capabilities ?? []).map((c) => ({ id: c, name: c })),
    imported: { objectives: i.objectives, problems: i.problems },
  };
}

/** The imported labs as last read (null until the first read answers). */
let labs: Lab[] | null = null;
let reading: Promise<void> | null = null;
const listeners = new Set<() => void>();

registerReset(() => {
  labs = null;
  reading = null;
});

function subscribe(cb: () => void) {
  listeners.add(cb);
  return () => {
    listeners.delete(cb);
  };
}

/** Reads the imported labs again (one read at a time), then each one's status. */
export function refreshImported(): Promise<void> {
  reading ??= importedLabs()
    .then((list) => {
      const next = list.map(importedLab);
      if (!structurallyEqual(labs, next)) {
        labs = next;
        for (const l of listeners) l();
      }
      next.forEach((l) => void refreshStatus(l));
    })
    .catch(
      warn("reading the labs imported from a folder", () => {
        if (labs === null) {
          labs = [];
          for (const l of listeners) l();
        }
      }),
    )
    .finally(() => {
      reading = null;
    });
  return reading;
}

/**
 * The labs imported from a folder installed here (null until first read). Read again whenever a
 * lab with an imported id starts deploying or shows up running or stops (one imported just now,
 * here or from another window), not on a timer. Not read at all while `enabled` is false.
 */
export function useImportedLabs(enabled = true): Lab[] | null {
  const shown = useSyncExternalStore(
    subscribe,
    () => labs,
    () => labs,
  );
  const deploying = useDeployingLabs(enabled);
  const scanned = useLabState("scanRunning");
  const key = [...deploying, ...scanned].filter(isImportedId).sort().join(",");
  useEffect(() => {
    if (enabled) void refreshImported();
  }, [enabled, key]);
  return shown;
}

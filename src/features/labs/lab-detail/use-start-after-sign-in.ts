"use client";

import { useState } from "react";
import type { LabDetailProps, LabPage } from "@/features/labs/lab-detail/use-lab-detail";

/**
 * "Sign in to start", carried through: once the sign-in it started went through, the start goes
 * on by itself, as the Start button would do it (where to run asked first when there is a
 * choice, else started here), instead of leaving the player in front of a Start button they
 * already pressed. Not when Start would be greyed out (no runtime, the lab busy) or a cloud lab
 * has no account yet: the page then shows why, as it would.
 */
export function useStartAfterSignIn(page: LabPage, p: LabDetailProps) {
  // Bumped when the start goes on with the "where" question: the Start button mounts again with
  // it open (`chooseAtOnce`), once per sign-in.
  const [tick, setTick] = useState(0);
  const { onLogin } = p;
  const blocked = !p.lab.runtime || p.busy || (page.s.isCloud && page.run.cloudAccounts.length === 0);
  const login = onLogin
    ? () =>
        onLogin().then((signedIn) => {
          if (signedIn && !blocked) {
            if (page.run.hasChoice) setTick((n) => n + 1);
            else page.startOn({ kind: "local" });
          }
          return signedIn;
        })
    : undefined;
  return { login, tick };
}

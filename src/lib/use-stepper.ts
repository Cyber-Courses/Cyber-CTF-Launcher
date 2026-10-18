"use client";

import { useState } from "react";

/** Where a wizard is among its `count` steps, and the moves between them (`goTo` jumps). The
 *  count can change (a step list that follows what the machine reports): the position stays
 *  within it. */
export function useStepper(count: number) {
  const [i, setI] = useState(0);
  return {
    pos: Math.min(i, count - 1),
    next: () => {
      setI((n) => Math.min(n + 1, count - 1));
    },
    back: () => {
      setI((n) => Math.max(n - 1, 0));
    },
    goTo: setI,
  };
}

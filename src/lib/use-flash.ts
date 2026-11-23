"use client";

import { useCallback, useEffect, useRef, useState } from "react";

/**
 * A flag shown for a moment (a "Copied" check): `flash()` turns it on, and it turns off again
 * `ms` later. Its timer goes with the component: an unmount clears it, and a flash after the
 * unmount (a clipboard answering late) does nothing, so no state is set once the component is
 * gone, nor after a test's environment is torn down.
 */
export function useFlash(ms: number): [boolean, () => void] {
  const [on, setOn] = useState(false);
  const timer = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);
  const alive = useRef(true);
  useEffect(() => {
    alive.current = true;
    return () => {
      alive.current = false;
      clearTimeout(timer.current);
    };
  }, []);
  const flash = useCallback(() => {
    if (!alive.current) return;
    clearTimeout(timer.current);
    setOn(true);
    timer.current = setTimeout(() => {
      setOn(false);
    }, ms);
  }, [ms]);
  return [on, flash];
}

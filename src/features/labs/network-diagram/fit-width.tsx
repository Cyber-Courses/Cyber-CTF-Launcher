"use client";

import { useEffect, useState } from "react";
import { ArrowRight, Expand, Shrink } from "lucide-react";
import { Button } from "@/components/ui/button";
import { useT } from "@/lib/i18n";

// A lab wider than its card: the diagram keeps a readable zoom and scrolls sideways (see
// `shellSize`). These are the card's controls for that case: a cue that there is more to the
// right, and a "Fit to width" toggle that shrinks the whole drawing into the card instead. The
// choice is remembered on this computer.

export const FIT_WIDTH_KEY = "cyberctf.diagram.fitWidth";

/** Below this the drawing is unreadable even fitted (React Flow's own minimum zoom, in flow.tsx). */
const FIT_MIN_ZOOM = 0.25;

/** Whether the player chose to fit wide diagrams to the card. Storage can be unavailable (a
 *  private window, blocked site data): then it is off. */
export function readFitWidth(): boolean {
  try {
    return localStorage.getItem(FIT_WIDTH_KEY) === "1";
  } catch {
    return false;
  }
}

export function writeFitWidth(on: boolean) {
  try {
    if (on) localStorage.setItem(FIT_WIDTH_KEY, "1");
    else localStorage.removeItem(FIT_WIDTH_KEY);
  } catch {
    // Not remembered, but it still applies until the page is left.
  }
}

/** The remembered choice, read once on mount (localStorage isn't available during render). */
export function useFitWidth() {
  const [fit, setFit] = useState(false);
  useEffect(() => {
    // eslint-disable-next-line react-hooks/set-state-in-effect -- read once on mount: localStorage doesn't exist during the static render
    setFit(readFitWidth());
  }, []);
  const set = (on: boolean) => {
    setFit(on);
    writeFitWidth(on);
  };
  return [fit, set] as const;
}

/** The shell's size for the graph when fitted: the canvas is the shell's width, and the drawing
 *  is scaled down to it (not below FIT_MIN_ZOOM), the height following it down (an overview
 *  needs less than `shellSize`'s 220px floor). Same margins as `shellSize`. */
export function fitShellSize(graph: { w: number; h: number } | null, width: number) {
  const scale = graph && width ? Math.max(FIT_MIN_ZOOM, Math.min(1.05, (width - 48) / Math.max(graph.w, 1))) : 1;
  const height = graph ? Math.round(Math.min(720, Math.max(140, graph.h * scale + 24))) : 430;
  return { canvas: width, height };
}

/** The header's controls, shown only for a lab wider than its card: "Scroll for more →" while
 *  there is more to the right (it scrolls there), and the Fit to width toggle. */
export function FitWidthControls({
  wide,
  fit,
  onFit,
  moreRight,
  onScrollMore,
}: {
  /** The drawing is wider than the card at its readable zoom. */
  wide: boolean;
  fit: boolean;
  onFit: (on: boolean) => void;
  /** Unfitted and not scrolled to the end. */
  moreRight: boolean;
  onScrollMore: () => void;
}) {
  const t = useT();
  if (!wide) return null;
  return (
    <span className="diagram-tools">
      {!fit && moreRight && (
        <button type="button" className="diagram-scroll-cue" onClick={onScrollMore} title={t("diagram.view.scrollMoreHint")}>
          {t("diagram.view.scrollMore")} <ArrowRight size={12} aria-hidden />
        </button>
      )}
      <Button
        variant="ghost"
        size="xs"
        className="aria-pressed:bg-glass-2 aria-pressed:text-foreground"
        aria-pressed={fit}
        title={t("diagram.view.fitWidthHint")}
        onClick={() => {
          onFit(!fit);
        }}
      >
        {fit ? <Expand className="size-3" aria-hidden /> : <Shrink className="size-3" aria-hidden />} {t("diagram.view.fitWidth")}
      </Button>
    </span>
  );
}

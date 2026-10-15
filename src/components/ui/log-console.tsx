"use client";

import { useEffect, useRef, useState } from "react";
import { useNow } from "@/lib/use-now";
import { Check, ChevronDown, ChevronRight, Loader2, X } from "lucide-react";
import { cn } from "@/lib/utils";
import { formatElapsed } from "@/lib/format";
import { useT } from "@/lib/i18n";

type LogConsoleProps = {
  lines: string[];
  running?: boolean;
  title?: string;
  /** Fold to its header once the run succeeds (it stays open on failure). */
  collapseOnDone?: boolean;
  className?: string;
};

/** Whether the log ends on a failure: a last line starting with ✗. */
const endsFailed = (lines: string[]) => lines.at(-1)?.trimStart().startsWith("✗") ?? false;

/**
 * Auto-scrolling terminal for streamed command output: black body, and a header with a
 * title, a live elapsed timer, and a collapse toggle. The timer ticks while `running` and
 * freezes when it ends, so the header doubles as done / failed feedback (failure is a final
 * line starting with ✗). The timer restarts each time `running` turns on, so a parent can
 * keep it mounted across runs.
 */
export function LogConsole({ lines, running = false, title: titleProp, collapseOnDone = false, className }: LogConsoleProps) {
  const t = useT();
  const title = titleProp ?? t("ui.log.output");
  const pre = useRef<HTMLPreElement>(null);
  const [open, setOpen] = useState(true);
  // Bumped as each run starts: its clock starts over.
  const [run, setRun] = useState(0);

  // Keep the newest line in view: scroll the log body itself to the bottom (not the page) on
  // every new line, so it reads like a tail -f and never needs manual scrolling.
  useEffect(() => {
    const el = pre.current;
    if (open && el) el.scrollTop = el.scrollHeight;
  }, [lines, open]);

  // Fold away a successful run when asked (its header still says it's done); keep failures open.
  // A new run restarts the timer and unfolds: callers that keep the console mounted between
  // runs (e.g. after a failure) would otherwise count from the first run's start.
  const [wasRunning, setWasRunning] = useState(running);
  if (wasRunning !== running) {
    setWasRunning(running);
    if (running) {
      setRun((r) => r + 1);
      setOpen(true);
    } else if (collapseOnDone && !endsFailed(lines)) setOpen(false);
  }

  if (lines.length === 0) return null;

  const failed = !running && endsFailed(lines);

  return (
    <div className={cn("surface-log overflow-hidden rounded-control", className)}>
      <button
        type="button"
        onClick={() => {
          setOpen((o) => !o);
        }}
        className="flex h-9 w-full items-center gap-2 border-b border-border px-3 text-left transition-colors hover:bg-accent"
      >
        {running ? (
          <Loader2 className="size-3.5 animate-spin text-jewel" />
        ) : failed ? (
          <X className="size-3.5 text-destructive" />
        ) : (
          <Check className="size-3.5 text-success" />
        )}
        <span className="text-[0.75rem] font-medium text-foreground">
          {running ? t("ui.log.running", { title }) : failed ? t("ui.log.failed", { title }) : t("ui.log.done", { title })}
        </span>
        <RunClock key={run} ticking={running} lines={lines.length} />
        {open ? <ChevronDown className="size-3.5 text-muted-foreground" /> : <ChevronRight className="size-3.5 text-muted-foreground" />}
      </button>
      {open && (
        <pre
          ref={pre}
          className="max-h-40 overflow-auto px-3.5 py-3 font-mono text-[0.75rem] leading-[1.8] break-words whitespace-pre-wrap text-muted-foreground"
        >
          {/* Tools print blank lines around their messages: trim them, keep one between paragraphs. */}
          {lines
            .join("\n")
            .replace(/^\s*\n+/, "")
            .replace(/\n\s*\n(\s*\n)+/g, "\n\n")
            .trimEnd()}
        </pre>
      )}
    </div>
  );
}

/** The header's elapsed time: from the run's first line, ticking every 300 ms while it runs and
 *  frozen once it ends. Its own component, so the tick re-renders only it, not the whole log. */
function RunClock({ ticking, lines }: { ticking: boolean; lines: number }) {
  const active = ticking && lines > 0;
  const [now, touch] = useNow(300, active);
  const [startAt, setStartAt] = useState<number | null>(null);
  useEffect(() => {
    if (!active) return;
    // At once, so a short run still shows its time.
    const first = setTimeout(() => {
      touch();
      setStartAt((s) => s ?? Date.now());
    }, 0);
    return () => {
      clearTimeout(first);
    };
  }, [active, lines, touch]);
  const elapsed = startAt === null ? 0 : Math.max(0, now - startAt);
  return <span className="ml-auto font-mono text-[0.6875rem] tabular-nums text-faint">{formatElapsed(elapsed)}</span>;
}

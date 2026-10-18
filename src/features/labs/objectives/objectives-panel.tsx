"use client";

import { useState } from "react";
import { ChevronRight, Circle, CircleCheck, Trophy } from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Panel, PanelHeader } from "@/components/ui/panel";
import { FORM, PROSE } from "@/components/ui/screen-width";
import { Spinner } from "@/components/ui/spinner";
import { AnswerForm } from "@/features/labs/objectives/answer-form";
import { GuidedHelp } from "@/features/labs/objectives/guided-help";
import { initiallyOpen, usedHelp, type Challenge, type LabProgress, type SubmitResult } from "@/features/labs/objectives/objectives";
import { useT } from "@/lib/i18n";
import { cn } from "@/lib/utils";

export interface ObjectiveActions {
  submit: (key: string, answer: string) => Promise<SubmitResult>;
  showSteps: (key: string) => Promise<void>;
  revealHint: (key: string, index: number) => Promise<void>;
}

/** Which rows are open (the ones `initial` returns at first), and a toggle for one. */
function useOpenRows(initial: () => string[]) {
  const [open, setOpen] = useState<Set<string>>(() => new Set(initial()));
  const toggle = (key: string) => {
    setOpen((s) => {
      const next = new Set(s);
      if (next.has(key)) next.delete(key);
      else next.add(key);
      return next;
    });
  };
  return [open, toggle] as const;
}

/** The header (required solved, or Completed), the progress bar and the completion note. */
function ObjectivesProgress({ progress }: { progress: LabProgress }) {
  const t = useT();
  const total = progress.requiredTotal;
  const done = Math.min(progress.requiredSolved, total);
  const pct = total ? Math.round((done / total) * 100) : 0;
  return (
    <>
      <PanelHeader
        title={t("labs.objectives.title")}
        meta={progress.completed ? undefined : t("labs.objectives.required", { solved: done, total })}
        action={
          progress.completed ? (
            <Badge variant="success" dot>
              {t("labs.objectives.completed")}
            </Badge>
          ) : undefined
        }
      />
      {total > 0 && (
        <div
          className="h-0.5 w-full bg-border"
          role="progressbar"
          aria-label={t("labs.objectives.progress")}
          aria-valuemin={0}
          aria-valuemax={total}
          aria-valuenow={done}
        >
          <div className="meter-fill h-full transition-[width] duration-500" style={{ width: `${String(pct)}%` }} />
        </div>
      )}
      {progress.completed && (
        <div className="flex items-center gap-2.5 border-b border-border px-4 py-3 text-[0.8125rem] text-foreground">
          <Trophy className="size-4 shrink-0 text-success" aria-hidden />
          {t("labs.objectives.completedNote")}
        </div>
      )}
    </>
  );
}

/**
 * The lab's challenges: solved state, optional ones, required progress and completion. One-shot
 * by default (the prompt and an answer field per challenge); guided on demand, per challenge:
 * its steps (each answered on its own) and its hints, revealed in order. Opening either is
 * recorded, and the panel says so before the first open.
 */
export function ObjectivesPanel({
  challenges,
  progress,
  signedIn,
  actions,
}: {
  challenges: Challenge[];
  progress: LabProgress;
  signedIn: boolean;
  actions: ObjectiveActions;
}) {
  const t = useT();
  const [open, toggle] = useOpenRows(() => initiallyOpen(challenges, progress.solved));
  return (
    <Panel>
      <ObjectivesProgress progress={progress} />
      {!signedIn && <p className="border-b border-border px-4 py-2.5 text-[0.75rem] text-muted-foreground">{t("labs.objectives.signIn")}</p>}
      <ol>
        {challenges.map((c) => (
          <ChallengeRow
            key={c.key}
            challenge={c}
            progress={progress}
            signedIn={signedIn}
            open={open.has(c.key)}
            onToggle={() => {
              toggle(c.key);
            }}
            actions={actions}
          />
        ))}
      </ol>
    </Panel>
  );
}

function ChallengeRow({
  challenge: c,
  progress,
  signedIn,
  open,
  onToggle,
  actions,
}: {
  challenge: Challenge;
  progress: LabProgress;
  signedIn: boolean;
  open: boolean;
  onToggle: () => void;
  actions: ObjectiveActions;
}) {
  const t = useT();
  const solved = progress.solved.includes(c.key);
  const guided = usedHelp(progress, c);
  const bodyId = `objective-${c.key}`;
  return (
    <li className="border-t border-border first:border-t-0">
      <button
        type="button"
        onClick={onToggle}
        aria-expanded={open}
        aria-controls={bodyId}
        className="flex w-full items-center gap-2.5 px-4 py-2.5 text-left transition-colors hover:bg-accent/40"
      >
        <ChevronRight className={cn("size-3.5 shrink-0 text-faint transition-transform", open && "rotate-90")} aria-hidden />
        {solved ? (
          <CircleCheck className="size-4 shrink-0 text-success" aria-label={t("labs.objectives.solved")} />
        ) : (
          <Circle className="size-4 shrink-0 text-faint" aria-hidden />
        )}
        <span className={cn("min-w-0 flex-1 truncate text-[0.8125rem]", solved ? "text-muted-foreground" : "text-foreground")}>{c.title}</span>
        {guided && <Badge>{t("labs.objectives.guided")}</Badge>}
        {c.optional && <Badge variant="outline">{t("labs.objectives.optional")}</Badge>}
      </button>
      {open && (
        <div id={bodyId} className={cn("space-y-3 px-4 pb-4 pl-[2.875rem]", FORM)}>
          {c.prompt && <p className={cn("text-[0.8125rem] leading-relaxed whitespace-pre-line text-foreground", PROSE)}>{c.prompt}</p>}
          <AnswerForm objectiveKey={c.key} title={c.title} solved={solved} signedIn={signedIn} submit={actions.submit} />
          <GuidedHelp challenge={c} progress={progress} signedIn={signedIn} actions={actions} />
        </div>
      )}
    </li>
  );
}

/** While the lab's objectives load: the lab's question, already known from the catalogue. */
export function ObjectivesLoading({ question }: { question: string }) {
  const t = useT();
  return (
    <Panel>
      <PanelHeader title={t("labs.objectives.title")} meta={<Spinner className="size-3.5" />} />
      <p className={cn("px-4 py-3.5 text-[0.8125rem] leading-relaxed whitespace-pre-line text-foreground", PROSE)}>{question}</p>
    </Panel>
  );
}

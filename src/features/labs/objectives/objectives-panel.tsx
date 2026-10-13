"use client";

import { useState, type FormEvent } from "react";
import { ChevronRight, Circle, CircleCheck, Lightbulb, ListChecks, Trophy } from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Panel, PanelHeader } from "@/components/ui/panel";
import { Spinner } from "@/components/ui/spinner";
import { StatusDot } from "@/components/ui/status-pill";
import {
  MAX_ANSWER,
  hintsOf,
  initiallyOpen,
  usedHelp,
  type Challenge,
  type LabProgress,
  type ObjectiveStep,
  type SubmitResult,
} from "@/features/labs/objectives/objectives";
import { useT } from "@/lib/i18n";
import { cn } from "@/lib/utils";

export interface ObjectiveActions {
  submit: (key: string, answer: string) => Promise<SubmitResult>;
  showSteps: (key: string) => Promise<void>;
  revealHint: (key: string, index: number) => Promise<void>;
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
  const [open, setOpen] = useState<Set<string>>(() => new Set(initiallyOpen(challenges, progress.solved)));
  const toggle = (key: string) =>
    setOpen((s) => {
      const next = new Set(s);
      if (next.has(key)) next.delete(key);
      else next.add(key);
      return next;
    });
  const total = progress.requiredTotal;
  const done = Math.min(progress.requiredSolved, total);
  const pct = total ? Math.round((done / total) * 100) : 0;
  return (
    <Panel>
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
          <div className="meter-fill h-full transition-[width] duration-500" style={{ width: `${pct}%` }} />
        </div>
      )}
      {progress.completed && (
        <div className="flex items-center gap-2.5 border-b border-border px-4 py-3 text-[0.8125rem] text-foreground">
          <Trophy className="size-4 shrink-0 text-success" aria-hidden />
          {t("labs.objectives.completedNote")}
        </div>
      )}
      {!signedIn && <p className="border-b border-border px-4 py-2.5 text-[0.75rem] text-muted-foreground">{t("labs.objectives.signIn")}</p>}
      <ol>
        {challenges.map((c) => (
          <ChallengeRow
            key={c.key}
            challenge={c}
            progress={progress}
            signedIn={signedIn}
            open={open.has(c.key)}
            onToggle={() => toggle(c.key)}
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
        <div id={bodyId} className="space-y-3 px-4 pb-4 pl-[2.875rem]">
          {c.prompt && <p className="text-[0.8125rem] leading-relaxed whitespace-pre-line text-foreground">{c.prompt}</p>}
          <AnswerForm objectiveKey={c.key} title={c.title} solved={solved} signedIn={signedIn} submit={actions.submit} />
          <GuidedHelp challenge={c} progress={progress} signedIn={signedIn} actions={actions} />
        </div>
      )}
    </li>
  );
}

type Feedback = { tone: "ok" | "warn" | "fail"; text: string } | null;

/** One answer field and its submit, for a challenge or a step (each by its own key). */
function AnswerForm({
  objectiveKey,
  title,
  solved,
  signedIn,
  submit,
}: {
  objectiveKey: string;
  title: string;
  solved: boolean;
  signedIn: boolean;
  submit: ObjectiveActions["submit"];
}) {
  const t = useT();
  const [answer, setAnswer] = useState("");
  const [checking, setChecking] = useState(false);
  const [feedback, setFeedback] = useState<Feedback>(null);

  async function onSubmit(e: FormEvent) {
    e.preventDefault();
    if (!answer.trim() || checking) return;
    setChecking(true);
    setFeedback(null);
    try {
      const r = await submit(objectiveKey, answer);
      if (r.alreadySolved) setFeedback({ tone: "ok", text: t("labs.objectives.alreadySolved") });
      else if (r.correct) setFeedback({ tone: "ok", text: r.labCompleted ? t("labs.objectives.correctCompleted") : t("labs.objectives.correct") });
      else setFeedback({ tone: "fail", text: t("labs.objectives.wrong") });
      if (r.correct) setAnswer("");
    } catch (err) {
      setFeedback({ tone: "fail", text: t("labs.objectives.failed", { error: String(err) }) });
    } finally {
      setChecking(false);
    }
  }

  const note = feedback && (
    <p role="status" className={cn("flex items-center gap-2 text-[0.75rem]", feedback.tone === "fail" ? "text-destructive" : "text-foreground")}>
      <StatusDot tone={feedback.tone} /> {feedback.text}
    </p>
  );

  // Solved: the form makes way for the result (what the last submit said, else just Solved).
  if (solved)
    return (
      note ?? (
        <p className="flex items-center gap-2 text-[0.75rem] text-muted-foreground">
          <StatusDot tone="ok" /> {t("labs.objectives.solved")}
        </p>
      )
    );

  return (
    <form onSubmit={onSubmit} className="space-y-2">
      <div className="flex gap-2">
        <Input
          fieldSize="sm"
          mono
          value={answer}
          onChange={(e) => setAnswer(e.target.value)}
          maxLength={MAX_ANSWER}
          placeholder={t("labs.objectives.answerPlaceholder")}
          aria-label={t("labs.objectives.answerFor", { title })}
          disabled={!signedIn || checking}
          autoComplete="off"
          spellCheck={false}
        />
        <Button type="submit" variant="outline" size="sm" disabled={!signedIn || checking || !answer.trim()}>
          {checking ? (
            <>
              <Spinner className="size-3.5" /> {t("labs.objectives.checking")}
            </>
          ) : (
            t("labs.objectives.submit")
          )}
        </Button>
      </div>
      {note}
    </form>
  );
}

/** Steps and hints of a challenge, on demand. */
function GuidedHelp({
  challenge: c,
  progress,
  signedIn,
  actions,
}: {
  challenge: Challenge;
  progress: LabProgress;
  signedIn: boolean;
  actions: ObjectiveActions;
}) {
  const t = useT();
  const [opening, setOpening] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const stepsOpen = progress.stepsOpened.includes(c.key);
  const hasSteps = c.steps.length > 0;
  if (!hasSteps && c.hintCount === 0) return null;
  const helpUsed = usedHelp(progress, c);

  async function openSteps() {
    setOpening(true);
    setError(null);
    try {
      await actions.showSteps(c.key);
    } catch (err) {
      setError(t("labs.objectives.helpFailed", { error: String(err) }));
    } finally {
      setOpening(false);
    }
  }

  return (
    <div className="space-y-3 border-t border-border pt-3">
      {!helpUsed && <p className="text-[0.6875rem] leading-relaxed text-muted-foreground">{t("labs.objectives.helpNote")}</p>}
      {hasSteps && !stepsOpen && (
        <Button variant="outline" size="xs" onClick={() => void openSteps()} disabled={!signedIn || opening} title={t("labs.objectives.guidedModeHint")}>
          {opening ? <Spinner className="size-3.5" /> : <ListChecks />} {t("labs.objectives.guidedMode")}
        </Button>
      )}
      {error && (
        <p className="flex items-center gap-2 text-[0.75rem] text-destructive">
          <StatusDot tone="fail" /> {error}
        </p>
      )}
      {hasSteps && stepsOpen && (
        <div className="space-y-2">
          <p className="section-label">{t("labs.objectives.steps")}</p>
          <ol className="space-y-3">
            {c.steps.map((s, i) => (
              <StepItem key={s.key} step={s} n={i + 1} progress={progress} signedIn={signedIn} actions={actions} />
            ))}
          </ol>
        </div>
      )}
      <Hints objective={c} progress={progress} signedIn={signedIn} reveal={actions.revealHint} />
    </div>
  );
}

function StepItem({
  step,
  n,
  progress,
  signedIn,
  actions,
}: {
  step: ObjectiveStep;
  n: number;
  progress: LabProgress;
  signedIn: boolean;
  actions: ObjectiveActions;
}) {
  const t = useT();
  const solved = progress.solved.includes(step.key);
  return (
    <li className="space-y-2 rounded-control p-3 shadow-[inset_0_0_0_1px_var(--border)]">
      <div className="flex items-center gap-2">
        {solved ? (
          <CircleCheck className="size-3.5 shrink-0 text-success" aria-label={t("labs.objectives.solved")} />
        ) : (
          <span className="font-mono text-[0.6875rem] text-faint">{t("labs.objectives.step", { n })}</span>
        )}
        <span className="min-w-0 text-[0.8125rem] font-medium text-foreground">{step.title}</span>
      </div>
      {step.prompt && <p className="text-[0.75rem] leading-relaxed whitespace-pre-line text-muted-foreground">{step.prompt}</p>}
      <AnswerForm objectiveKey={step.key} title={step.title} solved={solved} signedIn={signedIn} submit={actions.submit} />
      <Hints objective={step} progress={progress} signedIn={signedIn} reveal={actions.revealHint} />
    </li>
  );
}

/** An objective's opened hints, then a button for the next one (in order, one at a time). */
function Hints({
  objective,
  progress,
  signedIn,
  reveal,
}: {
  objective: ObjectiveStep;
  progress: LabProgress;
  signedIn: boolean;
  reveal: ObjectiveActions["revealHint"];
}) {
  const t = useT();
  const [opening, setOpening] = useState(false);
  const [error, setError] = useState<string | null>(null);
  if (objective.hintCount === 0) return null;
  const opened = hintsOf(progress, objective.key);
  const next = opened.length;

  async function onReveal() {
    setOpening(true);
    setError(null);
    try {
      await reveal(objective.key, next);
    } catch (err) {
      setError(t("labs.objectives.helpFailed", { error: String(err) }));
    } finally {
      setOpening(false);
    }
  }

  return (
    <div className="space-y-2">
      {opened.length > 0 && (
        <ol className="space-y-1.5">
          {opened.map((h) => (
            <li key={h.index} className="flex gap-2 text-[0.75rem] leading-relaxed text-foreground">
              <Lightbulb className="mt-0.5 size-3.5 shrink-0 text-warning" aria-hidden />
              <span>
                <span className="sr-only">{t("labs.objectives.hint", { n: h.index + 1 })}: </span>
                {h.text}
              </span>
            </li>
          ))}
        </ol>
      )}
      {next < objective.hintCount && (
        <Button variant="ghost" size="xs" onClick={() => void onReveal()} disabled={!signedIn || opening}>
          {opening ? <Spinner className="size-3.5" /> : <Lightbulb />} {t("labs.objectives.revealHint", { n: next + 1, total: objective.hintCount })}
        </Button>
      )}
      {error && (
        <p className="flex items-center gap-2 text-[0.75rem] text-destructive">
          <StatusDot tone="fail" /> {error}
        </p>
      )}
    </div>
  );
}

/** While the lab's objectives load: the lab's question, already known from the catalogue. */
export function ObjectivesLoading({ question }: { question: string }) {
  const t = useT();
  return (
    <Panel>
      <PanelHeader title={t("labs.objectives.title")} meta={<Spinner className="size-3.5" />} />
      <p className="px-4 py-3.5 text-[0.8125rem] leading-relaxed whitespace-pre-line text-foreground">{question}</p>
    </Panel>
  );
}

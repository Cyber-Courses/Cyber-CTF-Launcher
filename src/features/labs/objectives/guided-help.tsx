"use client";

import { useState } from "react";
import { CircleCheck, Lightbulb, ListChecks } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Spinner } from "@/components/ui/spinner";
import { StatusDot } from "@/components/ui/status-pill";
import { AnswerForm } from "@/features/labs/objectives/answer-form";
import { hintsOf, usedHelp, type Challenge, type LabProgress, type ObjectiveStep } from "@/features/labs/objectives/objectives";
import type { ObjectiveActions } from "@/features/labs/objectives/objectives-panel";
import { useT } from "@/lib/i18n";

// A challenge's guided help, on demand: its steps (each answered on its own) and its hints.

/** A help request (open the steps, reveal a hint): in flight, and what failed. */
function useHelpRequest() {
  const t = useT();
  const [opening, setOpening] = useState(false);
  const [error, setError] = useState<string | null>(null);
  async function request(open: () => Promise<void>) {
    setOpening(true);
    setError(null);
    try {
      await open();
    } catch (err) {
      setError(t("labs.objectives.helpFailed", { error: String(err) }));
    } finally {
      setOpening(false);
    }
  }
  return { opening, error, request };
}

function HelpError({ error }: { error: string | null }) {
  if (!error) return null;
  return (
    <p className="flex items-center gap-2 text-[0.75rem] text-destructive">
      <StatusDot tone="fail" /> {error}
    </p>
  );
}

/** Steps and hints of a challenge, on demand. */
export function GuidedHelp({
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
  const { opening, error, request } = useHelpRequest();
  const stepsOpen = progress.stepsOpened.includes(c.key);
  const hasSteps = c.steps.length > 0;
  if (!hasSteps && c.hintCount === 0) return null;
  const helpUsed = usedHelp(progress, c);
  return (
    <div className="space-y-3 border-t border-border pt-3">
      {!helpUsed && <p className="text-[0.6875rem] leading-relaxed text-muted-foreground">{t("labs.objectives.helpNote")}</p>}
      {hasSteps && !stepsOpen && (
        <Button
          variant="outline"
          size="xs"
          onClick={() => void request(() => actions.showSteps(c.key))}
          disabled={!signedIn || opening}
          title={t("labs.objectives.guidedModeHint")}
        >
          {opening ? <Spinner className="size-3.5" /> : <ListChecks />} {t("labs.objectives.guidedMode")}
        </Button>
      )}
      <HelpError error={error} />
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
  const { opening, error, request } = useHelpRequest();
  if (objective.hintCount === 0) return null;
  const opened = hintsOf(progress, objective.key);
  const next = opened.length;
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
        <Button variant="ghost" size="xs" onClick={() => void request(() => reveal(objective.key, next))} disabled={!signedIn || opening}>
          {opening ? <Spinner className="size-3.5" /> : <Lightbulb />} {t("labs.objectives.revealHint", { n: next + 1, total: objective.hintCount })}
        </Button>
      )}
      <HelpError error={error} />
    </div>
  );
}

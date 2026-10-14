"use client";

import { useState, type FormEvent } from "react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Spinner } from "@/components/ui/spinner";
import { StatusDot } from "@/components/ui/status-pill";
import { MAX_ANSWER, type SubmitResult } from "@/features/labs/objectives/objectives";
import { useT } from "@/lib/i18n";
import { cn } from "@/lib/utils";
import { describeError } from "@/lib/errors";

type Feedback = { tone: "ok" | "warn" | "fail"; text: string } | null;

/** An answer being typed and checked: what the last submit said, and the submit itself. */
function useAnswer(objectiveKey: string, submit: (key: string, answer: string) => Promise<SubmitResult>) {
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
      setFeedback({ tone: "fail", text: t("labs.objectives.failed", { error: describeError(err) }) });
    } finally {
      setChecking(false);
    }
  }
  return { answer, setAnswer, checking, feedback, onSubmit };
}

/** What the last submit said. */
function FeedbackNote({ feedback }: { feedback: NonNullable<Feedback> }) {
  return (
    <p role="status" className={cn("flex items-center gap-2 text-[0.75rem]", feedback.tone === "fail" ? "text-destructive" : "text-foreground")}>
      <StatusDot tone={feedback.tone} /> {feedback.text}
    </p>
  );
}

/** One answer field and its submit, for a challenge or a step (each by its own key). */
export function AnswerForm({
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
  submit: (key: string, answer: string) => Promise<SubmitResult>;
}) {
  const t = useT();
  const { answer, setAnswer, checking, feedback, onSubmit } = useAnswer(objectiveKey, submit);
  // Solved: the form makes way for the result (what the last submit said, else just Solved).
  if (solved)
    return feedback ? (
      <FeedbackNote feedback={feedback} />
    ) : (
      <p className="flex items-center gap-2 text-[0.75rem] text-muted-foreground">
        <StatusDot tone="ok" /> {t("labs.objectives.solved")}
      </p>
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
      {feedback && <FeedbackNote feedback={feedback} />}
    </form>
  );
}

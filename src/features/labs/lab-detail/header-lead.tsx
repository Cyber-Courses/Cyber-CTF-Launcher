"use client";

import { StatusStrip } from "@/components/ui/page-header";
import { Badge, LevelBadge } from "@/components/ui/badge";
import { StatusDot, type Tone } from "@/components/ui/status-pill";
import { AutoStop } from "@/features/labs/lab-timers";
import { runtimeWord, thisMachine } from "@/features/labs/lab-detail/labels";
import type { LabPhase, LabState } from "@/features/labs/lab-detail/lab-state";
import { difficultyLabel, type Lab } from "@/features/labs/use-labs";
import { OPERATION_STATUS } from "@/lib/deploy-store";
import type { LabStatus } from "@/lib/tauri";
import { useT, type T } from "@/lib/i18n";

/** The page's state as one dot and one word. */
function stateWord(t: T, s: LabState): { tone: Tone; word: string; pulse?: boolean } {
  const words: Record<Exclude<LabPhase, "operation">, { tone: Tone; word: string; pulse?: boolean }> = {
    running: { tone: "ok", word: t("labs.state.running") },
    starting: { tone: "warn", word: t("labs.state.starting"), pulse: true },
    paused: { tone: "muted", word: t("labs.state.paused") },
    shutDown: { tone: "muted", word: t("labs.state.shutDown") },
    interrupted: { tone: "warn", word: t("labs.state.interrupted") },
    notStarted: { tone: "muted", word: t("labs.state.notStarted") },
  };
  return s.phase === "operation" ? { tone: "warn", word: OPERATION_STATUS[s.operation!]!, pulse: true } : words[s.phase];
}

/** Under the title: the status strip (state, where, runtime, level, category), the skills and
 *  the description. */
export function HeaderLead({ lab, state: s, status }: { lab: Lab; state: LabState; status?: LabStatus }) {
  const t = useT();
  const state = stateWord(t, s);
  const strip: React.ReactNode[] = [
    <span key="state" className="inline-flex items-center gap-2">
      <StatusDot tone={state.tone} pulse={state.pulse} />
      <b>{state.word}</b>
    </span>,
  ];
  if (s.running) strip.push(<span key="where">{t("labs.detail.onWhere", { where: status?.host ?? thisMachine(t, s.engine) })}</span>);
  else if (s.parked && s.engine) strip.push(<span key="where">{s.engine}</span>);
  if (lab.runtime) strip.push(<span key="rt">{runtimeWord(t, s)}</span>);
  if (lab.runtime && !s.native && (s.isDocker || s.emulates))
    strip.push(
      <span key="emu" className="text-warning">
        {t("labs.detail.emulatedSlower")}
      </span>,
    );
  if (s.running && status?.expiresAt) strip.push(<AutoStop key="auto" at={status.expiresAt} />);
  if (lab.difficulty > 0)
    strip.push(
      <LevelBadge key="lvl" level={lab.difficulty}>
        {difficultyLabel(t, lab.difficulty)}
      </LevelBadge>,
    );
  strip.push(<span key="cat">{lab.category}</span>);

  return (
    <div className="space-y-2.5">
      <StatusStrip>
        {strip.map((node, i) => (
          <span key={i} className="inline-flex items-center gap-x-2.5">
            {i > 0 && <span aria-hidden>·</span>}
            {node}
          </span>
        ))}
      </StatusStrip>
      {(lab.skills ?? []).length > 0 && (
        <div className="flex flex-wrap gap-1.5">
          {lab.skills.map((sk) => (
            <Badge key={sk.id}>{sk.name}</Badge>
          ))}
        </div>
      )}
      {lab.description && <p className="max-w-2xl text-[0.8125rem] leading-relaxed text-muted-foreground">{lab.description}</p>}
    </div>
  );
}

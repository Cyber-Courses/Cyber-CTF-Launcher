"use client";

import { type DockerEngine } from "@/lib/tauri";
import { RadioList, RadioRow } from "@/components/ui/radio-row";
import { EngineLogo, RowPill } from "@/features/labs/run-on-options";
import { engineName } from "@/features/machine/setup-steps/engines";
import { useT } from "@/lib/i18n";

// The picker's step pieces: each step's heading, and the "Which engine?" step.

/** A step's title over the lab's name. */
export function PickerHeading({ heading, title }: { heading: string; title: string }) {
  return (
    <div>
      <p className="serif-title text-[1.5rem] text-foreground">{heading}</p>
      <p className="mt-1 text-[0.8125rem] text-muted-foreground">{title}</p>
    </div>
  );
}

/** "Which engine?": the container engines that answer here, as full rows (brand logo, name, the
 *  current one marked Default and preselected). */
export function EngineStep({
  title,
  engines,
  selected,
  fallback,
  onPick,
  disabled,
}: {
  title: string;
  engines: DockerEngine[];
  selected: DockerEngine | null;
  fallback: DockerEngine | null;
  onPick: (e: DockerEngine) => void;
  disabled: boolean;
}) {
  const t = useT();
  return (
    <div className="space-y-4">
      <PickerHeading heading={t("labs.runOn.engineHeading")} title={title} />
      <RadioList label={t("labs.runOn.engineHeading")}>
        {engines.map((e) => (
          <RadioRow
            key={e}
            selected={selected === e}
            onSelect={() => {
              onPick(e);
            }}
            disabled={disabled}
            title={engineName(e)}
            leading={<EngineLogo id={e} />}
            trailing={<RowPill tone="ok">{t(e === fallback ? "labs.runOn.defaultChoice" : "labs.runOn.running")}</RowPill>}
          />
        ))}
      </RadioList>
    </div>
  );
}

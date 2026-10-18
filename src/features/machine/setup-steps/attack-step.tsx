"use client";

import { useState } from "react";
import { Terminal } from "lucide-react";
import { ATTACK_PRESETS, DEFAULT_ATTACK_IMAGE, getAttackImage, getAutoAttackBox, setAttackImage, setAutoAttackBox } from "@/lib/settings";
import { type SystemReport } from "@/lib/tauri";
import { Choice, ChoiceGrid } from "@/components/ui/choice-card";
import { Switch } from "@/components/ui/switch";
import { TypeIcon } from "@/components/ui/type-icon";
import { isDockerReady } from "@/features/machine/setup-steps/steps";
import { formatBytes } from "@/lib/format";
import { useT, type T } from "@/lib/i18n";
import { useImageSizes } from "@/lib/use-image-sizes";
import { SharedFolderControl } from "@/features/settings/shared-folder";

type Preset = (typeof ATTACK_PRESETS)[number];

/** A preset's card note: what it is, its download size (or "large" when unknown), its terms. */
function presetNote(t: T, p: Preset, size: number | null | undefined): string {
  const download = size ? t("machine.attack.download", { size: formatBytes(size) }) : size === null && p.large ? t("machine.attack.largeDownload") : null;
  return [p.note, download, p.terms].filter(Boolean).join(" · ");
}

/** Pick the attack box image (same setting as Settings > Attack box) and whether it starts with each lab. */
export function AttackStep({ report }: { report: SystemReport }) {
  const t = useT();
  const [image, setImage] = useState(() => getAttackImage());
  const sizes = useImageSizes(ATTACK_PRESETS.map((p) => p.image));
  const custom = !ATTACK_PRESETS.some((p) => p.image === image);
  return (
    <div className="space-y-3">
      <ChoiceGrid>
        {ATTACK_PRESETS.map((p) => (
          <Choice
            key={p.image}
            selected={image === p.image}
            onSelect={() => {
              setAttackImage(p.image);
              setImage(p.image);
            }}
            mark={
              <TypeIcon>
                <Terminal className="size-4" />
              </TypeIcon>
            }
            title={p.label}
            note={presetNote(t, p, sizes[p.image])}
            badge={p.image === DEFAULT_ATTACK_IMAGE ? "recommended" : undefined}
          />
        ))}
      </ChoiceGrid>
      {custom && (
        <p className="text-left text-[0.75rem] text-muted-foreground">
          {t.rich("machine.attack.custom", { mono: (s) => <span className="font-mono text-[0.6875rem] text-foreground">{s}</span> }, { image })}
        </p>
      )}
      <AutoStartSwitch />
      <SharedFolderControl framed />
      {!isDockerReady(report) && <p className="text-left text-[0.75rem] text-muted-foreground">{t("machine.attack.needsEngine")}</p>}
      <p className="text-left text-[0.75rem] text-muted-foreground">{t("machine.attack.firstStart")}</p>
    </div>
  );
}

/** Whether the attack box starts with each lab. */
function AutoStartSwitch() {
  const t = useT();
  const [auto, setAuto] = useState(() => getAutoAttackBox());
  return (
    <div className="flex items-center justify-between gap-3 rounded-control bg-glass px-4 py-3 text-left shadow-[inset_0_0_0_1px_var(--border)]">
      <label htmlFor="attack-auto-start" className="min-w-0 cursor-pointer">
        <span className="block text-[0.8125rem] font-medium text-foreground">{t("machine.attack.autoStart")}</span>
        <span className="block text-[0.75rem] text-muted-foreground">{t("machine.attack.autoStartHint")}</span>
      </label>
      <Switch
        id="attack-auto-start"
        checked={auto}
        onCheckedChange={(on) => {
          setAutoAttackBox(on);
          setAuto(on);
        }}
      />
    </div>
  );
}

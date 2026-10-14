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
import { useT } from "@/lib/i18n";
import { useImageSizes } from "@/lib/use-image-sizes";
import { SharedFolderControl } from "@/features/settings/shared-folder";

// ---------- Virtual machines ----------

// ---------- Attack machine ----------

/** Pick the attack box image (same setting as Settings > Attack box) and whether it starts with each lab. */
export function AttackStep({ report }: { report: SystemReport }) {
  const t = useT();
  const [image, setImage] = useState(() => getAttackImage());
  const [auto, setAuto] = useState(() => getAutoAttackBox());
  const presets = ATTACK_PRESETS;
  const sizes = useImageSizes(presets.map((p) => p.image));
  const custom = !presets.some((p) => p.image === image);
  const pick = (img: string) => {
    setAttackImage(img);
    setImage(img);
  };
  return (
    <div className="space-y-3">
      <ChoiceGrid>
        {presets.map((p) => (
          <Choice
            key={p.image}
            selected={image === p.image}
            onSelect={() => pick(p.image)}
            mark={
              <TypeIcon>
                <Terminal className="size-4" />
              </TypeIcon>
            }
            title={p.label}
            note={[
              p.note,
              sizes[p.image]
                ? t("machine.attack.download", { size: formatBytes(sizes[p.image]!) })
                : sizes[p.image] === null && p.large
                  ? t("machine.attack.largeDownload")
                  : null,
              p.terms,
            ]
              .filter(Boolean)
              .join(" · ")}
            badge={p.image === DEFAULT_ATTACK_IMAGE ? "recommended" : undefined}
          />
        ))}
      </ChoiceGrid>
      {custom && (
        <p className="text-left text-[0.75rem] text-muted-foreground">
          {t.rich("machine.attack.custom", { mono: (s) => <span className="font-mono text-[0.6875rem] text-foreground">{s}</span> }, { image })}
        </p>
      )}
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
      <SharedFolderControl framed />
      {!isDockerReady(report) && <p className="text-left text-[0.75rem] text-muted-foreground">{t("machine.attack.needsEngine")}</p>}
      <p className="text-left text-[0.75rem] text-muted-foreground">{t("machine.attack.firstStart")}</p>
    </div>
  );
}

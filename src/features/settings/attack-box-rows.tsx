"use client";

import { useState } from "react";
import { Badge } from "@/components/ui/badge";
import { Switch } from "@/components/ui/switch";
import {
  ATTACK_PRESETS,
  CYBERCTF_ATTACK_REPO,
  DEFAULT_ATTACK_IMAGE,
  getAttackImage,
  getAutoAttackBox,
  setAttackImage,
  setAutoAttackBox,
  validImage,
} from "@/lib/settings";
import { Row } from "@/features/settings/settings-layout";
import { SharedFolderControl } from "@/features/settings/shared-folder";
import { RadioList, RadioRow } from "@/components/ui/radio-row";
import { formatBytes } from "@/lib/format";
import { useImageSizes } from "@/lib/use-image-sizes";
import { useT, type MessageKey } from "@/lib/i18n";
import { translated, usePresetChoice } from "@/features/settings/preset-choice";
import { AttackVmRow } from "@/features/settings/attack-vm-row";
import { CustomValueForm } from "@/features/settings/custom-value-form";

/** The presets' notes and terms, by image / box (a preset not listed keeps its English note). */
const IMAGE_NOTES: Partial<Record<string, MessageKey>> = {
  [DEFAULT_ATTACK_IMAGE]: "settings.attackBox.presets.cyberctf",
  "kalilinux/kali-rolling": "settings.attackBox.presets.kali",
  "parrotsec/security": "settings.attackBox.presets.parrot",
  "nwodtuhs/exegol:free": "settings.attackBox.presets.exegolFree",
  "nwodtuhs/exegol:full": "settings.attackBox.presets.exegolFull",
};
const TERMS: Partial<Record<string, MessageKey>> = {
  "Non-commercial use": "settings.attackBox.terms.nonCommercial",
  "Paid plan": "settings.attackBox.terms.paidPlan",
};

/* ------------------------------------------------------------------ labs */

export function AttackBoxRows({ onSaved }: { onSaved: () => void }) {
  return (
    <>
      <AttackImageRow onSaved={onSaved} />

      <AttackVmRow onSaved={onSaved} />

      <div className="border-t border-border px-4 py-3.5">
        <SharedFolderControl onSaved={onSaved} />
      </div>

      <AutoStartRow onSaved={onSaved} />
    </>
  );
}

/** The attack box image beside container labs: a preset, or any image the core accepts. */
function AttackImageRow({ onSaved }: { onSaved: () => void }) {
  const t = useT();
  const sizes = useImageSizes(ATTACK_PRESETS.map((p) => p.image));
  const {
    value: image,
    customOpen,
    setCustomOpen,
    draft,
    setDraft,
    choose,
  } = usePresetChoice(
    { read: getAttackImage, write: setAttackImage, fallback: DEFAULT_ATTACK_IMAGE },
    ATTACK_PRESETS.map((p) => p.image),
    onSaved,
  );

  const customDirty = draft.trim() !== "" && draft.trim() !== image && validImage(draft.trim());
  // Said under the field: a name the core refuses was saved before and failed only when a lab started.
  const customInvalid = draft.trim() !== "" && !validImage(draft.trim());

  return (
    <Row stacked title={t("settings.attackBox.title")} description={t("settings.attackBox.description")}>
      <RadioList label={t("settings.attackBox.title")} className="mt-3">
        {ATTACK_PRESETS.map((p) => (
          <RadioRow
            key={p.image}
            compact
            selected={!customOpen && image === p.image}
            onSelect={() => {
              setCustomOpen(false);
              choose(p.image);
            }}
            title={<ImagePresetTitle preset={p} size={sizes[p.image]} />}
            subtitle={<ImagePresetSubtitle preset={p} />}
          />
        ))}
        <RadioRow
          compact
          selected={customOpen}
          onSelect={() => {
            setCustomOpen(true);
          }}
          title={t("settings.attackBox.customImage")}
          subtitle={t("settings.attackBox.customImageNote")}
        >
          {customOpen && (
            <CustomValueForm
              draft={draft}
              onDraft={setDraft}
              dirty={customDirty}
              invalid={customInvalid}
              placeholder="registry/image:tag"
              onSave={() => {
                choose(draft);
              }}
            />
          )}
          {customOpen && customInvalid && <p className="mt-1.5 text-[0.75rem] text-destructive">{t("settings.attackBox.invalidImage")}</p>}
        </RadioRow>
      </RadioList>
    </Row>
  );
}

type ImagePreset = (typeof ATTACK_PRESETS)[number];

/** A preset's name with its badges: default, download size (or "Large download"), terms. */
function ImagePresetTitle({ preset: p, size }: { preset: ImagePreset; size: number | null | undefined }) {
  const t = useT();
  return (
    <>
      {p.label}
      {p.image === DEFAULT_ATTACK_IMAGE && <Badge>{t("settings.attackBox.default")}</Badge>}
      {size ? (
        <Badge variant="outline">{formatBytes(size)}</Badge>
      ) : (
        size === null && p.large && <Badge variant="outline">{t("settings.attackBox.largeDownload")}</Badge>
      )}
      {p.terms && <Badge variant="warning">{translated(t, TERMS[p.terms], p.terms)}</Badge>}
    </>
  );
}

/** A preset's image reference and note. */
function ImagePresetSubtitle({ preset: p }: { preset: ImagePreset }) {
  const t = useT();
  return (
    <>
      {p.image === DEFAULT_ATTACK_IMAGE ? (
        // The pinned default reads as its repository; the digest is in the tooltip.
        <span className="font-mono" title={p.image}>
          {CYBERCTF_ATTACK_REPO} ({t("settings.attackBox.pinned")})
        </span>
      ) : (
        <span className="font-mono">{p.image}</span>
      )}{" "}
      <span className="text-faint">·</span> {translated(t, IMAGE_NOTES[p.image], p.note)}
    </>
  );
}

/** Starts the attack box with each lab. */
function AutoStartRow({ onSaved }: { onSaved: () => void }) {
  const t = useT();
  const [autoStart, setAutoStart] = useState(() => getAutoAttackBox());
  return (
    <Row
      title={t("settings.attackBox.autoStart")}
      description={t("settings.attackBox.autoStartDescription")}
      control={
        <Switch
          aria-label={t("settings.attackBox.autoStart")}
          checked={autoStart}
          onCheckedChange={(on) => {
            setAutoAttackBox(on);
            setAutoStart(on);
            onSaved();
          }}
        />
      }
    />
  );
}

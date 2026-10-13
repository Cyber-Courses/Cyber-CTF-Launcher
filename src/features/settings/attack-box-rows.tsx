"use client";

import { useState } from "react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Switch } from "@/components/ui/switch";
import {
  ATTACK_PRESETS,
  ATTACK_VM_PRESETS,
  CYBERCTF_ATTACK_REPO,
  DEFAULT_ATTACK_BOX,
  DEFAULT_ATTACK_IMAGE,
  getAttackBox,
  getAttackImage,
  getAutoAttackBox,
  setAttackBox,
  setAttackImage,
  setAutoAttackBox,
} from "@/lib/settings";
import { Row } from "@/features/settings/settings-layout";
import { SharedFolderControl } from "@/features/settings/shared-folder";
import { RadioList, RadioRow } from "@/components/ui/radio-row";
import { formatBytes } from "@/lib/format";
import { useImageSizes } from "@/lib/use-image-sizes";
import { useT, type MessageKey } from "@/lib/i18n";

/** The presets' notes and terms, by image / box (a preset not listed keeps its English note). */
const IMAGE_NOTES: Record<string, MessageKey> = {
  [DEFAULT_ATTACK_IMAGE]: "settings.attackBox.presets.cyberctf",
  "kalilinux/kali-rolling": "settings.attackBox.presets.kali",
  "parrotsec/security": "settings.attackBox.presets.parrot",
  "nwodtuhs/exegol:free": "settings.attackBox.presets.exegolFree",
  "nwodtuhs/exegol:full": "settings.attackBox.presets.exegolFull",
};
const TERMS: Record<string, MessageKey> = {
  "Non-commercial use": "settings.attackBox.terms.nonCommercial",
  "Paid plan": "settings.attackBox.terms.paidPlan",
};
const BOX_NOTES: Record<string, MessageKey> = {
  "kalilinux/rolling": "settings.attackVm.presets.kali",
  "generic/debian12": "settings.attackVm.presets.debian12",
};

/* ------------------------------------------------------------------ labs */

export function AttackBoxRows({ onSaved }: { onSaved: () => void }) {
  const t = useT();
  const [image, setImage] = useState(() => getAttackImage());
  const isPreset = ATTACK_PRESETS.some((p) => p.image === image);
  const sizes = useImageSizes(ATTACK_PRESETS.map((p) => p.image));
  const [customOpen, setCustomOpen] = useState(!isPreset);
  const [draft, setDraft] = useState(isPreset ? "" : image);
  const [autoStart, setAutoStart] = useState(() => getAutoAttackBox());

  function choose(next: string) {
    const value = next.trim() || DEFAULT_ATTACK_IMAGE;
    setAttackImage(value);
    setImage(value);
    onSaved();
  }

  const customDirty = draft.trim() !== "" && draft.trim() !== image;

  return (
    <>
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
              title={
                <>
                  {p.label}
                  {p.image === DEFAULT_ATTACK_IMAGE && <Badge>{t("settings.attackBox.default")}</Badge>}
                  {sizes[p.image] ? (
                    <Badge variant="outline">{formatBytes(sizes[p.image]!)}</Badge>
                  ) : (
                    sizes[p.image] === null && p.large && <Badge variant="outline">{t("settings.attackBox.largeDownload")}</Badge>
                  )}
                  {p.terms && <Badge variant="warning">{TERMS[p.terms] ? t(TERMS[p.terms]) : p.terms}</Badge>}
                </>
              }
              subtitle={
                <>
                  {p.image === DEFAULT_ATTACK_IMAGE ? (
                    // The pinned default reads as its repository; the digest is in the tooltip.
                    <span className="font-mono" title={p.image}>
                      {CYBERCTF_ATTACK_REPO} ({t("settings.attackBox.pinned")})
                    </span>
                  ) : (
                    <span className="font-mono">{p.image}</span>
                  )}{" "}
                  <span className="text-faint">·</span> {IMAGE_NOTES[p.image] ? t(IMAGE_NOTES[p.image]) : p.note}
                </>
              }
            />
          ))}
          <RadioRow
            compact
            selected={customOpen}
            onSelect={() => setCustomOpen(true)}
            title={t("settings.attackBox.customImage")}
            subtitle={t("settings.attackBox.customImageNote")}
          >
            {customOpen && (
              <form
                className="mt-2.5 flex items-center gap-2"
                onSubmit={(e) => {
                  e.preventDefault();
                  if (customDirty) choose(draft);
                }}
              >
                <Input
                  fieldSize="sm"
                  mono
                  autoFocus
                  value={draft}
                  onChange={(e) => setDraft(e.target.value)}
                  onClick={(e) => e.stopPropagation()}
                  spellCheck={false}
                  placeholder="registry/image:tag"
                  className="min-w-0 flex-1"
                />
                <Button type="submit" variant="outline" size="xs" disabled={!customDirty} onClick={(e) => e.stopPropagation()}>
                  {t("settings.attackBox.save")}
                </Button>
              </form>
            )}
          </RadioRow>
        </RadioList>
      </Row>

      <AttackVmRow onSaved={onSaved} />

      <div className="border-t border-border px-4 py-3.5">
        <SharedFolderControl onSaved={onSaved} />
      </div>

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
    </>
  );
}

/** The attack VM beside VM labs: the learner's own Vagrant box, on the lab's hypervisor. */
function AttackVmRow({ onSaved }: { onSaved: () => void }) {
  const t = useT();
  const [box, setBox] = useState(() => getAttackBox());
  const isPreset = ATTACK_VM_PRESETS.some((p) => p.box === box);
  const [customOpen, setCustomOpen] = useState(!isPreset);
  const [draft, setDraft] = useState(isPreset ? "" : box);

  function choose(next: string) {
    const value = next.trim() || DEFAULT_ATTACK_BOX;
    setAttackBox(value);
    setBox(value);
    onSaved();
  }

  const customDirty = draft.trim() !== "" && draft.trim() !== box;

  return (
    <Row stacked title={t("settings.attackVm.title")} description={t("settings.attackVm.description")}>
      <RadioList label={t("settings.attackVm.label")} className="mt-3">
        {ATTACK_VM_PRESETS.map((p) => (
          <RadioRow
            key={p.box}
            compact
            selected={!customOpen && box === p.box}
            onSelect={() => {
              setCustomOpen(false);
              choose(p.box);
            }}
            title={
              <>
                {p.label}
                {p.box === DEFAULT_ATTACK_BOX && <Badge>{t("settings.attackBox.default")}</Badge>}
              </>
            }
            subtitle={
              <>
                <span className="font-mono">{p.box}</span> <span className="text-faint">·</span> {BOX_NOTES[p.box] ? t(BOX_NOTES[p.box]) : p.note}
              </>
            }
          />
        ))}
        <RadioRow
          compact
          selected={customOpen}
          onSelect={() => setCustomOpen(true)}
          title={t("settings.attackVm.customBox")}
          subtitle={t("settings.attackVm.customBoxNote")}
        >
          {customOpen && (
            <form
              className="mt-2.5 flex items-center gap-2"
              onSubmit={(e) => {
                e.preventDefault();
                if (customDirty) choose(draft);
              }}
            >
              <Input
                fieldSize="sm"
                mono
                autoFocus
                value={draft}
                onChange={(e) => setDraft(e.target.value)}
                onClick={(e) => e.stopPropagation()}
                spellCheck={false}
                placeholder="owner/name"
                className="min-w-0 flex-1"
              />
              <Button type="submit" variant="outline" size="xs" disabled={!customDirty} onClick={(e) => e.stopPropagation()}>
                {t("settings.attackBox.save")}
              </Button>
            </form>
          )}
        </RadioRow>
      </RadioList>
    </Row>
  );
}

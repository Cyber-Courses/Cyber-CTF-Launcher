"use client";

import { Badge } from "@/components/ui/badge";
import { RadioList, RadioRow } from "@/components/ui/radio-row";
import { translated, usePresetChoice } from "@/features/settings/preset-choice";
import { CustomValueForm } from "@/features/settings/custom-value-form";
import { Row } from "@/features/settings/settings-layout";
import { ATTACK_VM_PRESETS, DEFAULT_ATTACK_BOX, getAttackBox, setAttackBox } from "@/lib/settings";
import { useT, type MessageKey } from "@/lib/i18n";

/** The presets' notes, by box (a preset not listed keeps its English note). */
const BOX_NOTES: Partial<Record<string, MessageKey>> = {
  "kalilinux/rolling": "settings.attackVm.presets.kali",
  "generic/debian12": "settings.attackVm.presets.debian12",
};

/** The attack VM beside VM labs: the learner's own Vagrant box, on the lab's hypervisor. */
export function AttackVmRow({ onSaved }: { onSaved: () => void }) {
  const t = useT();
  const {
    value: box,
    customOpen,
    setCustomOpen,
    draft,
    setDraft,
    choose,
  } = usePresetChoice(
    { read: getAttackBox, write: setAttackBox, fallback: DEFAULT_ATTACK_BOX },
    ATTACK_VM_PRESETS.map((p) => p.box),
    onSaved,
  );
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
            title={<VmPresetTitle label={p.label} box={p.box} />}
            subtitle={
              <>
                <span className="font-mono">{p.box}</span> <span className="text-faint">·</span> {translated(t, BOX_NOTES[p.box], p.note)}
              </>
            }
          />
        ))}
        <RadioRow
          compact
          selected={customOpen}
          onSelect={() => {
            setCustomOpen(true);
          }}
          title={t("settings.attackVm.customBox")}
          subtitle={t("settings.attackVm.customBoxNote")}
        >
          {customOpen && (
            <CustomValueForm
              draft={draft}
              onDraft={setDraft}
              dirty={customDirty}
              placeholder="owner/name"
              onSave={() => {
                choose(draft);
              }}
            />
          )}
        </RadioRow>
      </RadioList>
    </Row>
  );
}

/** A preset box's name, marked when it is the default. */
function VmPresetTitle({ label, box }: { label: string; box: string }) {
  const t = useT();
  return (
    <>
      {label}
      {box === DEFAULT_ATTACK_BOX && <Badge>{t("settings.attackBox.default")}</Badge>}
    </>
  );
}

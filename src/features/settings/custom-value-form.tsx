"use client";

import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { useT } from "@/lib/i18n";

/** The free-text field of a "Custom" choice and its Save button (saves only a changed, valid value). */
export function CustomValueForm(props: {
  draft: string;
  onDraft: (v: string) => void;
  dirty: boolean;
  invalid?: boolean;
  placeholder: string;
  onSave: () => void;
}) {
  const t = useT();
  return (
    <form
      className="mt-2.5 flex items-center gap-2"
      onSubmit={(e) => {
        e.preventDefault();
        if (props.dirty) props.onSave();
      }}
    >
      <Input
        fieldSize="sm"
        mono
        autoFocus
        value={props.draft}
        onChange={(e) => {
          props.onDraft(e.target.value);
        }}
        onClick={(e) => {
          e.stopPropagation();
        }}
        spellCheck={false}
        placeholder={props.placeholder}
        aria-invalid={props.invalid || undefined}
        className="min-w-0 flex-1"
      />
      <Button
        type="submit"
        variant="outline"
        size="xs"
        disabled={!props.dirty}
        onClick={(e) => {
          e.stopPropagation();
        }}
      >
        {t("settings.attackBox.save")}
      </Button>
    </form>
  );
}

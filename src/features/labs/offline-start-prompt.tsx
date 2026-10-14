"use client";

import { ConfirmDialog } from "@/components/ui/confirm-dialog";
import { createAsk } from "@/lib/ask";
import { useT } from "@/lib/i18n";

// Cyber CTF can't be reached and the lab is installed here: start it anyway, without saving
// progress? `askOfflineStart` opens the dialog (mounted once, in the app shell) and resolves with
// the answer; closing it is a no.
const { ask, answer, useAsked } = createAsk<string, boolean>(false);

export const askOfflineStart = (title: string): Promise<boolean> => ask(title);

export function OfflineStartPrompt() {
  const t = useT();
  const asked = useAsked();
  if (!asked) return null;
  return (
    <ConfirmDialog
      key={asked.id}
      title={t("labs.offline.question", { title: asked.question })}
      confirmLabel={t("labs.offline.start")}
      onConfirm={() => answer(true)}
      onCancel={() => answer(false)}
    >
      {t("labs.offline.body")}
    </ConfirmDialog>
  );
}

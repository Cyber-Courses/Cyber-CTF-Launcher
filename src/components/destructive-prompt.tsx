"use client";

import { ConfirmDialog } from "@/components/ui/confirm-dialog";
import { destructive, warnings } from "@/lib/confirm";
import { useT } from "@/lib/i18n";

/** The question `confirmDestructive` asks, in the app's own dialog (mounted once, in the shell). */
export function DestructivePrompt() {
  const asked = destructive.useAsked();
  if (!asked) return null;
  const { title, message, confirm } = asked.question;
  return (
    <ConfirmDialog
      key={asked.id}
      title={title}
      confirmLabel={confirm}
      onConfirm={() => {
        destructive.answer(true);
      }}
      onCancel={() => {
        destructive.answer(false);
      }}
    >
      <span className="whitespace-pre-line">{message}</span>
    </ConfirmDialog>
  );
}

/** The warning `warnInDialog` shows, in the app's own dialog (mounted once, in the shell). */
export function WarningPrompt() {
  const t = useT();
  const asked = warnings.useAsked();
  if (!asked) return null;
  const { title, message } = asked.question;
  const close = () => {
    warnings.answer(null);
  };
  return (
    <ConfirmDialog key={asked.id} title={title} confirmLabel={t("ui.confirm.ok")} onConfirm={close} onCancel={close} acknowledge>
      <span className="whitespace-pre-line break-words">{message}</span>
    </ConfirmDialog>
  );
}

/** The app's own confirmation and warning dialogs, for a window's root: each window that runs
 *  code asking `confirmDestructive` or `warnInDialog` mounts them (the main window's shell, and
 *  the Settings and setup windows, which have roots of their own). */
export function DialogPrompts() {
  return (
    <>
      <DestructivePrompt />
      <WarningPrompt />
    </>
  );
}

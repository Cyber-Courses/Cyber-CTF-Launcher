"use client";

import { ConfirmDialog } from "@/components/ui/confirm-dialog";
import { useT } from "@/lib/i18n";

/** Stop & remove deletes the lab's machines: asked first. `canShutdown`: says Shut down keeps them. */
export function RemoveDialog({
  title,
  canShutdown,
  cloud = false,
  onCancel,
  onConfirm,
}: {
  title: string;
  canShutdown: boolean;
  /** A cloud lab: its services are destroyed in the player's account. */
  cloud?: boolean;
  onCancel: () => void;
  onConfirm: () => void;
}) {
  const t = useT();
  return (
    <ConfirmDialog title={t("labs.detail.removeTitle", { title })} confirmLabel={t("labs.detail.remove")} onCancel={onCancel} onConfirm={onConfirm}>
      {t(cloud ? "labs.detail.removeBodyCloud" : "labs.detail.removeBody")}
      {canShutdown && t("labs.detail.removeKeep")}
    </ConfirmDialog>
  );
}

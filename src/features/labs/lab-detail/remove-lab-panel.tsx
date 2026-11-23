"use client";

import { Trash2 } from "lucide-react";
import { Button } from "@/components/ui/button";
import { PanelHeader } from "@/components/ui/panel";
import { Spinner } from "@/components/ui/spinner";
import { Tip } from "@/components/ui/tip";
import { useT } from "@/lib/i18n";

/**
 * The lab's files on this computer, and Remove lab: deletes the app's copy of the lab and what it
 * keeps for it (never the folder an imported lab came from), once asked. Not while the lab is up,
 * parked, has machines left over, or an operation runs on it (`blocked`): stop it first.
 */
export function RemoveLabPanel({
  imported,
  blocked,
  removing,
  onRemove,
}: {
  imported: boolean;
  /** Something of the lab is up, or an operation runs on it. */
  blocked: boolean;
  /** The removal runs. */
  removing: boolean;
  onRemove: () => void;
}) {
  const t = useT();
  return (
    // A panel's surface, not clipped (`Panel` is): the tooltip under a blocked button reaches past
    // its edge.
    <section className="surface-panel min-w-0 rounded-panel">
      <PanelHeader title={t("labs.uninstall.title")} />
      <div className="space-y-2.5 p-4">
        <p className="text-[0.75rem] leading-relaxed text-muted-foreground">{t(imported ? "labs.uninstall.noteImported" : "labs.uninstall.noteCatalogue")}</p>
        <Tip text={blocked ? t("labs.uninstall.stopFirst") : undefined} align="start">
          <Button variant="destructive" size="sm" onClick={onRemove} disabled={blocked || removing}>
            {removing ? <Spinner className="size-3.5" /> : <Trash2 className="size-3.5" />}
            {removing ? t("labs.uninstall.removing") : t("labs.uninstall.button")}
          </Button>
        </Tip>
      </div>
    </section>
  );
}

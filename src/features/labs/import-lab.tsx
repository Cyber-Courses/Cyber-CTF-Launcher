"use client";

import { useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { Upload } from "lucide-react";
import { Button } from "@/components/ui/button";
import { labLaunchLocal } from "@/lib/tauri";
import { useT } from "@/lib/i18n";
import { tell } from "@/lib/failure";
import { notify } from "@/lib/notify";

/** Imports a lab from a local folder (one holding an isoloom.yml) and starts it, outside the
 *  catalogue and the backend: for running a lab that is not published yet (a lab author's own, a
 *  lab under test). The started lab appears in the running labs by its derived `local-<name>` id,
 *  driven by the ordinary lab controls. */
export function ImportLab() {
  const t = useT();
  const [busy, setBusy] = useState(false);
  const [step, setStep] = useState<string | null>(null);

  const pick = () => {
    open({ directory: true, multiple: false, title: t("labs.import.pick") })
      .then((picked) => {
        if (typeof picked !== "string") return;
        setBusy(true);
        setStep(null);
        return labLaunchLocal(picked, setStep)
          .then(() => notify(t("labs.import.started")))
          .catch(tell(t("labs.import.failed")))
          .finally(() => {
            setBusy(false);
            setStep(null);
          });
      })
      .catch(tell(t("labs.import.failed")));
  };

  return (
    <div className="flex min-w-0 items-center gap-2">
      {busy && step && <span className="hidden max-w-60 truncate font-mono text-[0.6875rem] text-muted-foreground md:inline">{step}</span>}
      <Button variant="outline" size="sm" onClick={pick} disabled={busy}>
        <Upload className="size-3.5" />
        {busy ? t("labs.import.importing") : t("labs.import.button")}
      </Button>
    </div>
  );
}

"use client";

import { useEffect, useState } from "react";
import { Switch } from "@/components/ui/switch";
import { Row } from "@/features/settings/settings-layout";
import { isoloomRegistryGet, isoloomRegistrySet } from "@/lib/tauri";
import { useT } from "@/lib/i18n";
import { warn } from "@/lib/failure";
import { describeError } from "@/lib/errors";

/** Whether the labs started here are recorded in Isoloom's registry (`~/.isoloom/status.yml`),
 *  so the `isoloom` command line sees them. On by default; labs work the same without it. */
export function IsoloomRegistryRow({ onSaved }: { onSaved: () => void }) {
  const t = useT();
  const [on, setOn] = useState<boolean | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    isoloomRegistryGet().then(setOn).catch(warn("reading the Isoloom registry setting"));
  }, []);

  if (on === null) return null;
  return (
    <Row
      title={t("settings.isoloomRegistry.title")}
      description={t("settings.isoloomRegistry.description")}
      control={
        <Switch
          aria-label={t("settings.isoloomRegistry.title")}
          checked={on}
          onCheckedChange={(next) => {
            setError(null);
            isoloomRegistrySet(next)
              .then((saved) => {
                setOn(saved);
                onSaved();
              })
              .catch((e) => setError(describeError(e)));
          }}
        />
      }
    >
      {error && <p className="mt-2 text-[0.75rem] text-destructive">{error}</p>}
    </Row>
  );
}

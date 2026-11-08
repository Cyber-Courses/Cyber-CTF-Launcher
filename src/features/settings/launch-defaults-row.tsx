"use client";

import { useState } from "react";
import { Button } from "@/components/ui/button";
import { getDockerEngine, getLocalVmForm, getVmProvider, setDockerEngine, setLocalVmForm, setVmProvider } from "@/lib/settings";
import { Row } from "@/features/settings/settings-layout";
import { useT } from "@/lib/i18n";

/** The engine and VM form "don't ask again" remembered for launches here (the host ports have their
 *  own row): shown, with a reset so the dialog asks again. */
export function LaunchDefaultsRow({ onSaved }: { onSaved: () => void }) {
  const t = useT();
  const [tick, setTick] = useState(0);
  // Read each render so a reset (or a dialog's "don't ask again") shows at once.
  const remembered = tick >= 0 && (getDockerEngine() !== null || getLocalVmForm() !== null || getVmProvider() !== null);
  return (
    <Row
      title={t("settings.launchDefaults.title")}
      description={remembered ? t("settings.launchDefaults.remembered") : t("settings.launchDefaults.asked")}
      control={
        <Button
          variant="ghost"
          size="sm"
          disabled={!remembered}
          onClick={() => {
            // The host ports have their own row; this forgets the engine, VM form and hypervisor.
            setDockerEngine(null);
            setLocalVmForm(null);
            setVmProvider(null);
            setTick((n) => n + 1);
            onSaved();
          }}
        >
          {t("settings.launchDefaults.reset")}
        </Button>
      }
    />
  );
}

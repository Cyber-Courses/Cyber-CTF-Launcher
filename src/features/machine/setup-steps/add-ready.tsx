"use client";

import { useState } from "react";
import { Button } from "@/components/ui/button";
import { ChoiceAction } from "@/components/ui/choice-card";
import { SelfTest, type SelfTestResult, type SelfTestTarget } from "@/features/machine/self-test";
import { useT } from "@/lib/i18n";

/** A provider the picker installed (or found): `note`, its own test (inline, on `target`), and
 *  once the test passes, "Use by default" unless it is the default already. */
export function ProviderReady({
  note,
  kind,
  target,
  canTest = true,
  isDefault,
  busy = false,
  onUseDefault,
}: {
  note: string;
  kind: "docker" | "vm";
  target: SelfTestTarget;
  /** False while the provider is there but not running: nothing to test yet. */
  canTest?: boolean;
  isDefault: boolean;
  busy?: boolean;
  onUseDefault: () => void;
}) {
  const t = useT();
  const [testing, setTesting] = useState(false);
  const [result, setResult] = useState<SelfTestResult>("idle");
  return (
    <>
      <ChoiceAction>
        <span className="text-[0.8125rem] text-muted-foreground">{note}</span>
        <span className="flex shrink-0 gap-2">
          {result === "ok" && !isDefault && (
            <Button variant="primary" size="sm" disabled={busy} onClick={onUseDefault}>
              {t("machine.screen.useDefault")}
            </Button>
          )}
          {canTest && (
            <Button
              variant={result === "ok" ? "outline" : "primary"}
              size="sm"
              onClick={() => {
                setTesting((on) => !on);
              }}
            >
              {testing ? t("machine.screen.hideTest") : t("machine.screen.test")}
            </Button>
          )}
        </span>
      </ChoiceAction>
      {testing && (
        <SelfTest
          kind={kind}
          title={kind === "docker" ? t("machine.selfTest.containerTitle") : t("machine.selfTest.vmTitle")}
          description={kind === "docker" ? t("machine.selfTest.machineDockerDescription") : t("machine.selfTest.machineVmDescription")}
          auto
          target={target}
          onResult={setResult}
        />
      )}
    </>
  );
}

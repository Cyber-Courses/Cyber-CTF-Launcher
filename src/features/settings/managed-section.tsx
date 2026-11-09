"use client";

import { Badge } from "@/components/ui/badge";
import { Row, Section } from "@/features/settings/settings-layout";
import type { PolicySetting } from "@/lib/tauri";
import { useT } from "@/lib/i18n";

/** What this computer's administrator turned off (the policy switches that are on), each with the
 *  setting that did: nothing when the policy turns nothing off. */
export function ManagedSection({ policy }: { policy: readonly PolicySetting[] }) {
  const t = useT();
  if (policy.length === 0) return null;
  return (
    <Section title={t("settings.managed.title")} description={t("settings.managed.description")}>
      {policy.map((setting) => (
        <Row
          key={setting}
          title={t(`settings.managed.keys.${setting}`)}
          description={<span className="font-mono">{setting}</span>}
          control={<Badge variant="warning">{t("settings.managed.off")}</Badge>}
        />
      ))}
    </Section>
  );
}

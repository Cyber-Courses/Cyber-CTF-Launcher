"use client";

import { SecretValue } from "@/components/ui/secret-value";
import { useT } from "@/lib/i18n";

/** The attack box's password for its player account: masked until Show, copied with one click.
 *  The lab page's Credentials row and the box's screen window show it. */
export function PasswordValue({ password }: { password: string }) {
  const t = useT();
  return (
    <SecretValue
      text={password}
      labels={{ show: t("labs.attackBox.showPassword"), hide: t("labs.attackBox.hidePassword"), copy: t("labs.attackBox.copyPassword") }}
    />
  );
}

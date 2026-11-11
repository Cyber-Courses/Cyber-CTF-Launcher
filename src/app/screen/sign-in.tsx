"use client";

import { useEffect, useState } from "react";
import { KeyRound } from "lucide-react";
import { Button } from "@/components/ui/button";
import { CopyValue } from "@/components/ui/copy-value";
import { PasswordValue } from "@/features/labs/password-value";
import { warn } from "@/lib/failure";
import { useT } from "@/lib/i18n";
import { screenSignIn, type SignIn } from "@/lib/tauri";
import { useEscape } from "@/lib/use-escape";

/** The attack box's sign-in, in its screen window's toolbar: where the screen asks for a password
 *  (the screen locker, LightDM's or the console's login), the account and its password in a
 *  popover, the password masked until Show. Nothing for any other machine, nor for a box the
 *  launcher keeps no password for. */
export function SignInPopover() {
  const t = useT();
  const [signIn, setSignIn] = useState<SignIn | null>(null);
  const [open, setOpen] = useState(false);
  useEffect(() => {
    let live = true;
    screenSignIn()
      .then((s) => {
        if (live) setSignIn(s);
      })
      .catch(warn("the attack box's sign-in couldn't be read"));
    return () => {
      live = false;
    };
  }, []);
  const close = () => {
    setOpen(false);
  };
  useEscape(close, open);
  if (!signIn) return null;
  return (
    <div className="relative">
      <Button
        variant="ghost"
        size="xs"
        aria-haspopup="dialog"
        aria-expanded={open}
        onClick={() => {
          setOpen((o) => !o);
        }}
      >
        <KeyRound className="size-3.5" /> {t("shell.vmScreen.signIn")}
      </Button>
      {open && (
        <>
          <button type="button" aria-hidden tabIndex={-1} className="fixed inset-0 z-40 cursor-default" onClick={close} />
          <div
            role="dialog"
            aria-label={t("shell.vmScreen.signInTitle")}
            className="surface-glass absolute top-full right-0 z-50 mt-1.5 w-72 space-y-2.5 rounded-control p-3.5 text-[0.75rem]"
          >
            <p className="font-medium text-foreground">{t("shell.vmScreen.signInTitle")}</p>
            <p className="leading-relaxed text-muted-foreground">{t("shell.vmScreen.signInHint", { user: signIn.user })}</p>
            <dl className="grid grid-cols-[auto_1fr] items-center gap-x-3 gap-y-1.5">
              <dt className="text-muted-foreground">{t("shell.vmScreen.user")}</dt>
              <dd>
                <CopyValue text={signIn.user} />
              </dd>
              <dt className="text-muted-foreground">{t("shell.vmScreen.password")}</dt>
              <dd>
                <PasswordValue password={signIn.password} />
              </dd>
            </dl>
          </div>
        </>
      )}
    </div>
  );
}

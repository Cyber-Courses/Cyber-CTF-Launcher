"use client";

import { getCurrentWindow } from "@tauri-apps/api/window";
import { RotateCw } from "lucide-react";
import { Button } from "@/components/ui/button";
import { warn } from "@/lib/failure";
import { useT } from "@/lib/i18n";

type Props = {
  /** The session failed (said in the destructive colour), rather than ended. */
  failed: boolean;
  /** Why, translated. */
  message: string;
  /** The way back in (Reconnect, Try again), translated. */
  retry: string;
  onRetry: () => void;
};

/** Along the bottom of a shell or screen window whose session ended or failed: why, the way back
 *  in, and Close. */
export function WindowEndedBar({ failed, message, retry, onRetry }: Props) {
  const t = useT();
  return (
    <div className="absolute inset-x-0 bottom-0 flex items-center justify-between gap-3 border-t border-border bg-background/95 px-4 py-2.5 text-[0.8125rem]">
      <span className={failed ? "text-destructive" : "text-muted-foreground"}>{message}</span>
      <div className="flex gap-2">
        <Button variant="outline" size="sm" onClick={onRetry}>
          <RotateCw className="size-3.5" /> {retry}
        </Button>
        <Button
          variant="ghost"
          size="sm"
          onClick={() => {
            getCurrentWindow().close().catch(warn("closing the window"));
          }}
        >
          {t("shell.terminal.close")}
        </Button>
      </div>
    </div>
  );
}

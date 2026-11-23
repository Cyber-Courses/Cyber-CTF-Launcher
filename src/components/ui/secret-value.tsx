"use client";

import { useState } from "react";
import { Eye, EyeOff } from "lucide-react";
import { CopyValue } from "@/components/ui/copy-value";

/** What a hidden secret shows in its place. */
const MASK = "••••••••";

/** A secret (a password, a key): masked until Show, copied with one click either way, the copy's
 *  tooltip never showing it while it is masked. `labels` name the buttons, for their tooltips and
 *  screen readers. */
export function SecretValue({ text, labels }: { text: string; labels: { show: string; hide: string; copy: string } }) {
  const [shown, setShown] = useState(false);
  return (
    <span className="inline-flex min-w-0 items-center gap-1.5">
      <CopyValue text={text} label={shown ? undefined : MASK} title={shown ? undefined : labels.copy} />
      <button
        type="button"
        onClick={() => {
          setShown((s) => !s);
        }}
        className="text-faint transition-colors hover:text-foreground"
        aria-label={shown ? labels.hide : labels.show}
        title={shown ? labels.hide : labels.show}
      >
        {shown ? <EyeOff className="size-3.5" /> : <Eye className="size-3.5" />}
      </button>
    </span>
  );
}

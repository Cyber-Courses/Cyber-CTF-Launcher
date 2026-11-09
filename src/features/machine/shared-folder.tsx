"use client";

import { useEffect, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { FolderOpen } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Switch } from "@/components/ui/switch";
import { sharedFolderGet, sharedFolderOpen, sharedFolderSet, type SharedFolder } from "@/lib/tauri";
import { useT } from "@/lib/i18n";
import { warn } from "@/lib/failure";
import { describeError } from "@/lib/errors";

/** The shared folder setting: read once, saved on each change, its folder picked or opened; a
 *  failure (a denied permission, a path refused) is kept to show. */
function useSharedFolder(onSaved?: () => void) {
  const t = useT();
  const [state, setState] = useState<SharedFolder | null>(null);
  const [error, setError] = useState<string | null>(null);
  const fail = (e: unknown) => {
    setError(describeError(e));
  };

  useEffect(() => {
    sharedFolderGet().then(setState).catch(warn("reading the shared folder setting"));
  }, []);

  const save = (enabled: boolean, path: string) => {
    setError(null);
    sharedFolderSet(enabled, path)
      .then((next) => {
        setState(next);
        onSaved?.();
      })
      .catch(fail);
  };
  // The picker couldn't open (a denied permission): said, not dropped.
  const choose = () => {
    open({ directory: true, multiple: false, defaultPath: state?.path || undefined, title: t("settings.sharedFolder.pick") })
      .then((picked) => {
        if (typeof picked === "string") save(true, picked);
      })
      .catch(fail);
  };
  const openFolder = () => {
    sharedFolderOpen().catch(fail);
  };
  return { state, error, save, choose, openFolder };
}

/** The folder shared with the attack box: on/off, which folder, and a way to open it. Used in
 *  Settings and in the setup's attack box step (the same setting). */
export function SharedFolderControl({ onSaved, framed = false }: { onSaved?: () => void; framed?: boolean }) {
  const t = useT();
  const { state, error, save, choose, openFolder } = useSharedFolder(onSaved);

  if (!state) return null;
  return (
    <div className={framed ? "rounded-control bg-glass px-4 py-3 text-left shadow-[inset_0_0_0_1px_var(--border)]" : ""}>
      <div className="flex items-center justify-between gap-3">
        <label htmlFor="shared-folder-on" className="min-w-0 cursor-pointer">
          <span className="block text-[0.8125rem] font-medium text-foreground">{t("settings.sharedFolder.title")}</span>
          <span className="block text-[0.75rem] text-muted-foreground">{t("settings.sharedFolder.description", { mount: state.mountPoint })}</span>
        </label>
        <Switch
          id="shared-folder-on"
          checked={state.enabled}
          onCheckedChange={(on) => {
            save(on, state.path);
          }}
        />
      </div>
      {state.enabled && (
        <div className="mt-2.5 flex items-center gap-2">
          <span
            className="min-w-0 flex-1 truncate rounded-control bg-background px-2.5 py-1.5 font-mono text-[0.6875rem] text-foreground shadow-[inset_0_0_0_1px_var(--border)]"
            title={state.path}
          >
            {state.path}
          </span>
          <Button variant="outline" size="xs" onClick={choose}>
            {t("settings.sharedFolder.change")}
          </Button>
          <Button variant="ghost" size="xs" onClick={openFolder} title={t("settings.sharedFolder.open")}>
            <FolderOpen className="size-3.5" />
          </Button>
        </div>
      )}
      {error && <p className="mt-2 text-[0.75rem] text-destructive">{error}</p>}
    </div>
  );
}

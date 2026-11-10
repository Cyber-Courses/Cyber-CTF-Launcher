"use client";

import { Suspense, useEffect, useMemo, useState } from "react";
import { useSearchParams } from "next/navigation";
import { emit } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { HostSetupPage, SetupTrademarks } from "@/features/servers/host-setup";
import { startingSetup } from "@/features/servers/host-setup/setup-model";
import { Spinner } from "@/components/ui/spinner";
import { Toaster } from "@/components/ui/toaster";
import { WindowTitleBar } from "@/components/window-title-bar";
import { SERVER_CHANGED, isCloudProvider, serverList, systemCheck, type ServerHostInput, type SystemReport } from "@/lib/tauri";
import { warn } from "@/lib/failure";
import { useT } from "@/lib/i18n";
import { DevMockGate } from "@/components/dev-mock-gate";

/** The server setup window (opened by `server_open_setup`), closed when setup ends. */
function Setup() {
  const t = useT();
  const params = useSearchParams();
  const id = params.get("id");
  const { form: empty, providerChosen } = useMemo(() => startingSetup(params.get("kind"), params.get("provider")), [params]);
  const initial = useInitialHost(id, empty);
  const [report, setReport] = useState<SystemReport | null>(null);

  const check = () => {
    systemCheck().then(setReport).catch(warn("system check"));
  };
  useEffect(check, []);

  const close = () => {
    getCurrentWindow().close().catch(warn("closing the window"));
  };

  const cloud = id ? !!initial && isCloudProvider(initial.provider) : params.get("kind") === "cloud";

  return (
    <div className="flex h-screen flex-col bg-background text-foreground">
      <WindowTitleBar />

      {!initial ? (
        <div className="flex flex-1 items-center justify-center gap-2 pb-11 text-[0.8125rem] text-muted-foreground">
          <Spinner className="size-4" /> {t("servers.setup.loading")}
        </div>
      ) : (
        /* The wizard fills the window's height: its title stays at the top and each step's Nav
           (Back / Continue) is pinned to the bottom of this scroller, so the buttons are always in
           view and don't move from step to step; a tall step scrolls between them. The
           trademark is pinned below. */
        <div className="min-h-0 flex-1 overflow-y-auto">
          <main className="mx-auto flex min-h-full w-full max-w-[38rem] flex-col px-6 pt-6">
            <div className="flex w-full flex-1 flex-col">
              <HostSetupPage
                initial={initial}
                providerChosen={!id && providerChosen}
                report={report}
                onRefresh={check}
                onSaved={() => {
                  emit(SERVER_CHANGED).catch(warn("telling the main window a server changed"));
                }}
                onDone={close}
              />
            </div>
          </main>
        </div>
      )}

      {initial && (
        <div className="shrink-0 border-t border-border px-6 py-3">
          <SetupTrademarks cloud={cloud} />
        </div>
      )}
      <Toaster />
    </div>
  );
}

/** The form the setup starts from: `empty` for a new host, else host `id` as saved (its password
 *  kept), or `empty` when it isn't found; null while it loads. */
function useInitialHost(id: string | null, empty: ServerHostInput) {
  const [initial, setInitial] = useState<ServerHostInput | null>(id ? null : empty);
  useEffect(() => {
    if (!id) return;
    serverList()
      .then((l) => {
        const h = l.hosts.find((x) => x.id === id);
        setInitial(h ? { ...h, password: null } : { ...empty });
      })
      .catch(
        warn("reading the saved servers to edit one", () => {
          setInitial({ ...empty });
        }),
      );
  }, [id, empty]);
  return initial;
}

export default function ServerSetupWindow() {
  return (
    <DevMockGate>
      <Suspense>
        <Setup />
      </Suspense>
    </DevMockGate>
  );
}

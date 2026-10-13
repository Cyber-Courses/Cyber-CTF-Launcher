"use client";

import { Suspense, useEffect, useState } from "react";
import { useSearchParams } from "next/navigation";
import { emit } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { HostSetupPage, SetupTrademarks, EMPTY_CLOUD, EMPTY_HOST } from "@/features/servers/host-setup";
import { Spinner } from "@/components/ui/spinner";
import { Toaster } from "@/components/ui/toaster";
import { SERVER_CHANGED, serverList, systemCheck, type ServerHostInput, type SystemReport } from "@/lib/tauri";
import { warn } from "@/lib/failure";
import { useT } from "@/lib/i18n";
import { DevMockGate } from "@/components/dev-mock-gate";

/** The server setup window (opened by `server_open_setup`), closed when setup ends. */
function Setup() {
  const t = useT();
  const params = useSearchParams();
  const id = params.get("id");
  const empty = params.get("kind") === "cloud" ? EMPTY_CLOUD : EMPTY_HOST;
  const [initial, setInitial] = useState<ServerHostInput | null>(id ? null : { ...empty });
  const [report, setReport] = useState<SystemReport | null>(null);

  const check = () => {
    systemCheck().then(setReport).catch(warn("system check"));
  };
  useEffect(check, []);
  useEffect(() => {
    if (!id) return;
    serverList()
      .then((l) => {
        const h = l.hosts.find((x) => x.id === id);
        setInitial(h ? { ...h, password: null } : { ...empty });
      })
      .catch(() => setInitial({ ...empty }));
  }, [id, empty]);

  const close = () => getCurrentWindow().close().catch(warn("closing the window"));

  const cloud = id ? initial?.provider === "aws" || initial?.provider === "azure" || initial?.provider === "gcp" : params.get("kind") === "cloud";

  return (
    <div className="flex h-screen flex-col bg-background text-foreground">
      {/* Overlay title bar: drags the window and clears the macOS traffic lights. */}
      <div data-tauri-drag-region className="h-11 shrink-0 select-none" />

      {!initial ? (
        <div className="flex flex-1 items-center justify-center gap-2 pb-11 text-[0.8125rem] text-muted-foreground">
          <Spinner className="size-4" /> {t("servers.setup.loading")}
        </div>
      ) : (
        /* The wizard centers in the window; the trademark is pinned at the bottom so it
           doesn't unbalance the centering. Scrolls when a step is tall. */
        <div className="min-h-0 flex-1 overflow-y-auto">
          {/* my-auto centers a short step but collapses to 0 when the step is tall, so the
              top never clips and the window scrolls (unlike justify-center on a flex child). */}
          <main className="mx-auto flex min-h-full w-full max-w-[38rem] flex-col px-6 py-6">
            <div className="my-auto w-full">
              <HostSetupPage
                initial={initial}
                report={report}
                onRefresh={check}
                onSaved={() => emit(SERVER_CHANGED).catch(warn("telling the main window a server changed"))}
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

export default function ServerSetupWindow() {
  return (
    <DevMockGate>
      <Suspense>
        <Setup />
      </Suspense>
    </DevMockGate>
  );
}

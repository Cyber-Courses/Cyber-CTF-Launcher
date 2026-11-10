"use client";

import { useCallback, useEffect, useRef, useState } from "react";
import { ChevronRight, Trash2 } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Panel, PanelHeader } from "@/components/ui/panel";
import { Spinner } from "@/components/ui/spinner";
import { ListSkeleton } from "@/features/machine/machine-parts";
import { formatBytes } from "@/lib/format";
import { translate, useT } from "@/lib/i18n";
import { tell, warn } from "@/lib/failure";
import { confirmDestructive } from "@/lib/confirm";
import { describeError } from "@/lib/errors";
import { getAttackImage, OLD_ATTACK_IMAGES } from "@/lib/settings";
import { machineStorage, machineStorageClean, type Storage } from "@/lib/tauri";
import { cn } from "@/lib/utils";

/** The attack box images to count: the one in use, and an older launcher's default (several GB,
 *  by tag or earlier digest) so it can be cleaned up. The bare repo is `:latest` again. Only the
 *  Cyber CTF image's are the app's by name; another image counts only if the app pulled it. */
function attackImages(): string[] {
  return [getAttackImage(), ...OLD_ATTACK_IMAGES.filter((i) => i.includes(":") || i.includes("@"))];
}

/** What Cyber CTF downloaded (null until read, or when the read failed: `readError` says why),
 *  and its clean-up, once confirmed: what it freed. */
function useDownloads() {
  const [storage, setStorage] = useState<Storage | null>(null);
  const [readError, setReadError] = useState<string | null>(null);
  const [cleaning, setCleaning] = useState(false);
  const [freed, setFreed] = useState<number | null>(null);
  const alive = useRef(true);
  useEffect(() => {
    // Set on every mount: React's dev StrictMode unmounts and remounts once, and the cleanup
    // alone left this false for good (the panel never left its skeleton).
    alive.current = true;
    return () => {
      alive.current = false;
    };
  }, []);
  const load = useCallback(() => {
    machineStorage(attackImages()).then(
      (s) => {
        if (!alive.current) return;
        setStorage(s);
        setReadError(null);
      },
      (e: unknown) => {
        // Never "nothing downloaded": what is stored is unknown, and the panel says why.
        warn("reading what Cyber CTF downloaded")(e);
        if (!alive.current) return;
        setStorage(null);
        setReadError(describeError(e));
      },
    );
  }, []);
  useEffect(() => {
    load();
  }, [load]);

  async function clean(total: number) {
    const yes = await confirmDestructive({
      title: translate("machine.downloads.confirmTitle", { size: formatBytes(total) }),
      message: translate("machine.downloads.confirmBody"),
      confirm: translate("machine.downloads.remove"),
    });
    if (!yes) return;
    setCleaning(true);
    // A failed clean-up is told, and nothing reads as freed.
    setFreed(
      await machineStorageClean(attackImages()).catch((e: unknown) => {
        tell(translate("machine.downloads.cleanFailed"))(e);
        return null;
      }),
    );
    setCleaning(false);
    load();
  }
  return {
    storage,
    readError,
    cleaning,
    freed,
    clean: (total: number) => {
      void clean(total);
    },
  };
}

/** The container images and VM boxes Cyber CTF downloaded itself, with a confirmed clean-up
 *  (anything in use is kept; the player's own images and boxes are never listed). */
export function DownloadsPanel() {
  const t = useT();
  const { storage, readError, cleaning, freed, clean } = useDownloads();
  const storeItems = storage ? [...storage.images, ...storage.boxes, ...storage.plugins] : [];
  const storeTotal = storeItems.reduce((a, i) => a + i.bytes, 0);
  return (
    <Panel>
      <PanelHeader
        title={t("machine.downloads.title")}
        meta={storage && storeItems.length > 0 ? <span className="tabular-nums">{formatBytes(storeTotal)}</span> : undefined}
        action={
          storage && storeItems.length > 0 ? (
            <Button
              variant="outline"
              size="xs"
              onClick={() => {
                clean(storeTotal);
              }}
              disabled={cleaning}
            >
              {cleaning ? (
                <>
                  <Spinner className="size-3" /> {t("machine.downloads.removing")}
                </>
              ) : (
                <>
                  <Trash2 /> {t("machine.downloads.cleanUp")}
                </>
              )}
            </Button>
          ) : undefined
        }
      />
      {storage === null && readError !== null ? (
        <p className="px-4 py-3 text-[0.8125rem] text-destructive">{t("machine.downloads.readFailed", { why: readError })}</p>
      ) : storage === null ? (
        <ListSkeleton />
      ) : (
        <>
          <StorageList storage={storage} items={storeItems} total={storeTotal} />
          {freed !== null && (
            <p className="border-t border-border px-4 py-2.5 text-[0.75rem] text-muted-foreground">
              {freed > 0 ? t("machine.downloads.freed", { size: formatBytes(freed) }) : t("machine.downloads.nothingFreed")}
            </p>
          )}
        </>
      )}
    </Panel>
  );
}

type StoreItem = Storage["images"][number];

/** The total, by kind, opening onto every item with its size. */
function StorageList({ storage, items, total }: { storage: Storage; items: StoreItem[]; total: number }) {
  const t = useT();
  const [open, setOpen] = useState(false);
  return (
    <>
      <button
        onClick={() => {
          setOpen((v) => !v);
        }}
        disabled={items.length === 0}
        aria-expanded={open}
        className="flex min-h-[3.25rem] w-full items-center gap-3 px-4 py-2 text-left transition-colors enabled:hover:bg-glass"
      >
        <ChevronRight className={cn("size-3.5 shrink-0 text-muted-foreground transition-transform", open && "rotate-90", items.length === 0 && "opacity-0")} />
        <div className="min-w-0 flex-1">
          <p className="text-[0.8125rem] font-medium text-foreground">
            {items.length ? t("machine.downloads.total", { size: formatBytes(total) }) : t("machine.downloads.empty")}
          </p>
          <p className="font-mono text-[0.6875rem] text-faint">
            {t("machine.downloads.containerImages", { count: storage.images.length })} · {t("machine.downloads.vmImages", { count: storage.boxes.length })}
          </p>
        </div>
      </button>
      {open &&
        items.map((it) => (
          <div key={it.name} className="flex items-center gap-3 border-t border-border py-2 pr-4 pl-10 transition-colors hover:bg-glass">
            <span className="min-w-0 flex-1 truncate font-mono text-[0.6875rem] text-muted-foreground">{it.name}</span>
            <span className="shrink-0 font-mono text-[0.6875rem] tabular-nums text-faint">{formatBytes(it.bytes)}</span>
          </div>
        ))}
    </>
  );
}

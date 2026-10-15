"use client";

import { useCallback, useEffect, useRef, useState } from "react";
import { ChevronRight, Trash2 } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Panel, PanelHeader } from "@/components/ui/panel";
import { Spinner } from "@/components/ui/spinner";
import { StatusDot } from "@/components/ui/status-pill";
import { ListSkeleton } from "@/features/machine/machine-parts";
import { formatBytes } from "@/lib/format";
import { useT } from "@/lib/i18n";
import { getAttackImage, OLD_ATTACK_IMAGES } from "@/lib/settings";
import { machineStorage, machineStorageClean, type Storage } from "@/lib/tauri";
import { cn } from "@/lib/utils";

/** The attack box images to count: the one in use, and an older launcher's default (several GB,
 *  by tag or earlier digest) so it can be cleaned up. The bare repo is `:latest` again. Only the
 *  Cyber CTF image's are the app's by name; another image counts only if the app pulled it. */
function attackImages(): string[] {
  return [getAttackImage(), ...OLD_ATTACK_IMAGES.filter((i) => i.includes(":") || i.includes("@"))];
}

/** Nothing downloaded, or nothing the storage read could see. */
const NO_STORAGE: Storage = { images: [], boxes: [], plugins: [] };

/** What Cyber CTF downloaded (null until read), and its confirmed clean-up: what it freed. */
function useDownloads() {
  const [storage, setStorage] = useState<Storage | null>(null);
  const [confirmClean, setConfirmClean] = useState(false);
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
    const show = (s: Storage) => {
      if (alive.current) setStorage(s);
    };
    machineStorage(attackImages()).then(show, () => {
      show(NO_STORAGE);
    });
  }, []);
  useEffect(() => {
    load();
  }, [load]);

  async function clean() {
    setCleaning(true);
    setFreed(await machineStorageClean(attackImages()).catch(() => 0));
    setCleaning(false);
    setConfirmClean(false);
    load();
  }
  return {
    storage,
    confirmClean,
    setConfirmClean,
    cleaning,
    freed,
    clean: () => {
      void clean();
    },
  };
}

/** The container images and VM boxes Cyber CTF downloaded itself, with a confirmed clean-up
 *  (anything in use is kept; the player's own images and boxes are never listed). */
export function DownloadsPanel() {
  const t = useT();
  const { storage, confirmClean, setConfirmClean, cleaning, freed, clean } = useDownloads();
  const storeItems = storage ? [...storage.images, ...storage.boxes, ...storage.plugins] : [];
  const storeTotal = storeItems.reduce((a, i) => a + i.bytes, 0);
  return (
    <Panel>
      <PanelHeader
        title={t("machine.downloads.title")}
        meta={storage && storeItems.length > 0 ? <span className="tabular-nums">{formatBytes(storeTotal)}</span> : undefined}
        action={
          storage && storeItems.length > 0 && !confirmClean ? (
            <Button
              variant="outline"
              size="xs"
              onClick={() => {
                setConfirmClean(true);
              }}
              disabled={cleaning}
            >
              <Trash2 /> {t("machine.downloads.cleanUp")}
            </Button>
          ) : undefined
        }
      />
      {storage === null ? (
        <ListSkeleton />
      ) : (
        <>
          <StorageList storage={storage} items={storeItems} total={storeTotal} />
          {confirmClean && (
            <CleanConfirm
              total={storeTotal}
              cleaning={cleaning}
              onCancel={() => {
                setConfirmClean(false);
              }}
              onConfirm={clean}
            />
          )}
          {freed !== null && !confirmClean && (
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

/** "Remove N GB?" with Cancel and Remove (a spinner while it runs). */
function CleanConfirm({ total, cleaning, onCancel, onConfirm }: { total: number; cleaning: boolean; onCancel: () => void; onConfirm: () => void }) {
  const t = useT();
  return (
    <div role="alert" className="flex flex-wrap items-center gap-3 border-t border-border bg-glass px-4 py-3">
      <StatusDot tone="warn" />
      <p className="min-w-0 flex-1 text-[0.75rem] text-foreground">{t("machine.downloads.confirm", { size: formatBytes(total) })}</p>
      <div className="flex gap-2">
        <Button variant="ghost" size="xs" onClick={onCancel} disabled={cleaning}>
          {t("machine.downloads.cancel")}
        </Button>
        <Button variant="destructive" size="xs" onClick={onConfirm} disabled={cleaning}>
          {cleaning ? (
            <>
              <Spinner className="size-3" /> {t("machine.downloads.removing")}
            </>
          ) : (
            t("machine.downloads.remove")
          )}
        </Button>
      </div>
    </div>
  );
}

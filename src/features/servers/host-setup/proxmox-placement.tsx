"use client";

import { useEffect, useRef, useState } from "react";
import { Spinner } from "@/components/ui/spinner";
import { Field, Input, Note, Select } from "@/features/servers/host-setup/form";
import { placementWith } from "@/features/servers/host-setup/setup-model";
import type { HostSetup } from "@/features/servers/host-setup/use-host-setup";
import { serverProxmoxPlacement, type ProxmoxNodeChoices, type ProxmoxPlacement } from "@/lib/tauri";
import { describeError } from "@/lib/errors";
import { useT } from "@/lib/i18n";

// Proxmox's placement: the server's own nodes, storages and bridges, read from its API once
// signed in, each preselected; typed by hand only when the API can't be read.

type Read = { state: "reading" } | { state: "read"; placement: ProxmoxPlacement } | { state: "failed"; error: string };

/** Reads the server's choices once, and fills the form's empty or unknown placement with the
 *  defaults. */
function usePlacementChoices(s: HostSetup) {
  // Read once, with the sign-in the Connection step left: later edits don't read again.
  const first = useRef(s);
  const [read, setRead] = useState<Read>({ state: "reading" });
  useEffect(() => {
    const { payload, setV } = first.current;
    let live = true;
    serverProxmoxPlacement(payload)
      .then((placement) => {
        if (!live) return;
        if (placement.nodes.length === 0) {
          setRead({ state: "failed", error: "no node" });
          return;
        }
        setRead({ state: "read", placement });
        setV((v) => placementWith(v, placement, v.node ?? null));
      })
      .catch((err: unknown) => {
        if (live) setRead({ state: "failed", error: describeError(err) });
      });
    return () => {
      live = false;
    };
  }, []);
  return read;
}

export function ProxmoxPlacementFields({ s }: { s: HostSetup }) {
  const t = useT();
  const read = usePlacementChoices(s);
  if (read.state === "reading")
    return (
      <p className="flex items-center gap-2 text-[0.8125rem] text-muted-foreground">
        <Spinner className="size-4" /> {t("servers.setup.placement.reading")}
      </p>
    );
  if (read.state === "failed") return <ManualPlacement s={s} error={read.error} />;
  const node = read.placement.nodes.find((n) => n.name === s.v.node) ?? read.placement.nodes[0];
  return <PlacementChoices s={s} placement={read.placement} node={node} />;
}

/** One drop-down per choice, the node's options in it. */
function PlacementChoices({ s, placement, node }: { s: HostSetup; placement: ProxmoxPlacement; node: ProxmoxNodeChoices }) {
  const t = useT();
  const { v, set, setV } = s;
  const pick = (k: "datastore" | "imageStorage" | "snippetStorage" | "network") => (e: React.ChangeEvent<HTMLSelectElement>) => {
    set(k, e.target.value);
  };
  return (
    <div className="grid gap-3 sm:grid-cols-2">
      <Field label={t("servers.setup.placement.node")}>
        <Select
          value={node.name}
          onChange={(e) => {
            setV((x) => placementWith(x, placement, e.target.value, true));
          }}
        >
          {placement.nodes.map((n) => (
            <option key={n.name}>{n.name}</option>
          ))}
        </Select>
      </Field>
      <Field label={t("servers.setup.placement.bridge")} hint={t("servers.setup.placement.bridgeHint")}>
        <Select value={v.network ?? ""} onChange={pick("network")}>
          {node.bridges.map((b) => (
            <option key={b.name} value={b.name}>
              {b.name} · {b.cidr ?? t("servers.setup.placement.noAddress")}
            </option>
          ))}
        </Select>
      </Field>
      <StorageSelect label={t("servers.setup.placement.diskStorage")} value={v.datastore} options={node.diskStorages} onChange={pick("datastore")} />
      <StorageSelect label={t("servers.setup.placement.imageStorage")} value={v.imageStorage} options={node.imageStorages} onChange={pick("imageStorage")} />
      <StorageSelect
        label={t("servers.setup.placement.snippetStorage")}
        value={v.snippetStorage}
        options={node.snippetStorages}
        onChange={pick("snippetStorage")}
      />
    </div>
  );
}

function StorageSelect({
  label,
  value,
  options,
  onChange,
}: {
  label: string;
  value: string | null | undefined;
  options: string[];
  onChange: (e: React.ChangeEvent<HTMLSelectElement>) => void;
}) {
  const t = useT();
  return (
    <Field label={label} hint={options.length === 0 ? t("servers.setup.placement.noStorage") : undefined}>
      <Select value={value ?? ""} onChange={onChange} disabled={options.length === 0}>
        {options.map((o) => (
          <option key={o}>{o}</option>
        ))}
      </Select>
    </Field>
  );
}

/** The API couldn't be read: why, and the fields to type by hand (blank = the defaults). */
function ManualPlacement({ s, error }: { s: HostSetup; error: string }) {
  const t = useT();
  const { text } = s;
  return (
    <>
      <Note className="mb-3 text-muted-foreground">{t("servers.setup.placement.readFailed", { error })}</Note>
      <div className="grid gap-3 sm:grid-cols-3">
        <Field label={t("servers.setup.placement.node")} hint={t("servers.setup.placement.optional")}>
          <Input {...text("node")} placeholder="pve" />
        </Field>
        <Field label={t("servers.setup.placement.diskStorage")} hint={t("servers.setup.placement.optional")}>
          <Input {...text("datastore")} placeholder="local-lvm" />
        </Field>
        <Field label={t("servers.setup.placement.bridge")} hint={t("servers.setup.placement.optional")}>
          <Input {...text("network")} placeholder="vmbr0" />
        </Field>
        <Field label={t("servers.setup.placement.imageStorage")} hint={t("servers.setup.placement.optional")}>
          <Input {...text("imageStorage")} placeholder="local" />
        </Field>
        <Field label={t("servers.setup.placement.snippetStorage")} hint={t("servers.setup.placement.optional")}>
          <Input {...text("snippetStorage")} placeholder="local" />
        </Field>
      </div>
    </>
  );
}

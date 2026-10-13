import { useState } from "react";
import { serverSave, type CloudProvider, type ServerHost, type ServerHostInput, type ServerTest, type SystemReport } from "@/lib/tauri";
import { KIND } from "@/features/servers/host-setup/constants";
import {
  connectionReady,
  keepsSecret,
  isCliAuthCloud,
  isCloud,
  isTokenCloud,
  machineTools,
  saveName,
  setupSteps,
  setupTitle,
  withProvider,
} from "@/features/servers/host-setup/setup-model";
import { useCloudAccounts } from "@/features/servers/host-setup/use-cloud-accounts";
import { useRunLog } from "@/features/servers/host-setup/use-run-log";
import { testHost } from "@/features/servers/use-server-list";
import { translate, useT } from "@/lib/i18n";

/** All the server / cloud setup state and actions, shared by the setup steps. */
export function useHostSetup({
  initial,
  report,
  onRefresh,
  onSaved,
  onDone,
}: {
  initial: ServerHostInput;
  report: SystemReport | null;
  onRefresh: () => void;
  /** After every save, so the main window's host list can refresh. */
  onSaved: (h: ServerHost) => void;
  /** Setup finished or cancelled: the window closes. */
  onDone: () => void;
}) {
  const t = useT();
  const [v, setV] = useState<ServerHostInput>(initial);
  const [i, setI] = useState(0);
  const editing = initial.id !== null;
  const cloud = isCloud(v.provider);
  // Derived from the saved provider (not separate state) so it stays correct when editing an
  // existing account, where the provider step that would set it is skipped.
  const cloudProvider: CloudProvider = isCloud(v.provider) ? v.provider : "aws";
  const accounts = useCloudAccounts(v, setV, editing, cloudProvider);

  const steps = setupSteps(v.provider, editing);
  const key = steps[Math.min(i, steps.length - 1)];
  const next = () => setI((n) => Math.min(n + 1, steps.length - 1));
  const back = () => setI((n) => Math.max(n - 1, 0));

  const set = <K extends keyof ServerHostInput>(k: K, value: ServerHostInput[K]) => setV((s) => ({ ...s, [k]: value }));
  const text = (k: "name" | "host" | "username" | "datastore" | "network" | "node") => ({
    value: (v[k] as string | null) ?? "",
    onChange: (e: React.ChangeEvent<HTMLInputElement>) => set(k, e.target.value),
  });
  const pickProvider = (id: CloudProvider) => setV((s) => withProvider(s, id));

  const tools = useToolInstall(onRefresh);
  const { saving, error, saved, test, runTest, saveAndTest } = useSaveAndTest(v, onSaved, next);
  const title = setupTitle({ saved, test, editing, cloud, initialName: initial.name });

  return {
    report,
    onRefresh,
    onDone,
    v,
    setV,
    i,
    saving,
    error,
    saved,
    test,
    editing,
    cloud,
    cloudProvider,
    aws: v.provider === "aws",
    azure: v.provider === "azure",
    gcp: v.provider === "gcp",
    digitalocean: v.provider === "digitalocean",
    linode: v.provider === "linode",
    oci: v.provider === "oci",
    tokenCloud: isTokenCloud(v.provider),
    cliAuth: isCliAuthCloud(v.provider),
    kind: KIND[v.provider],
    ...accounts,
    pickProvider,
    steps,
    key,
    set,
    text,
    connectionOk: connectionReady(v, keepsSecret(initial, v)),
    secretKept: keepsSecret(initial, v),
    next,
    back,
    ...tools,
    ...machineTools(v.provider, report),
    runTest,
    saveAndTest,
    title: t(`servers.setup.titles.${title.key}`, { name: title.name }),
  };
}

/** Installs one of the tools this server type needs on this machine, logging below, then
 *  re-reads what the machine has. */
function useToolInstall(onRefresh: () => void) {
  const toolLog = useRunLog();
  const [toolLabel, setToolLabel] = useState("Install");
  // Set for the whole install: an installer printing its own "✓ …" line mid-run read as done
  // from the log, and a second install could start beside the first.
  const [toolBusy, setToolBusy] = useState(false);
  async function installTool(label: string, run: (onLog: (l: string) => void) => Promise<void>) {
    if (toolBusy) return;
    setToolLabel(label);
    setToolBusy(true);
    try {
      await toolLog.run(translate("servers.setup.tools.installing", { name: label }), async (onLog) => {
        await run(onLog);
        return translate("servers.setup.tools.installDone");
      });
    } finally {
      setToolBusy(false);
    }
    onRefresh();
  }
  return { pluginLog: toolLog.lines, toolLabel, toolBusy, installTool };
}

/** Saving the host, then moving to the Test step (`next`) and testing the saved host. */
function useSaveAndTest(v: ServerHostInput, onSaved: (h: ServerHost) => void, next: () => void) {
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [saved, setSaved] = useState<ServerHost | null>(null);
  const [test, setTest] = useState<ServerTest | "testing" | null>(null);

  async function runTest(id: string) {
    setTest("testing");
    setTest(await testHost(id));
  }

  async function saveAndTest() {
    setSaving(true);
    setError(null);
    try {
      const h = await serverSave({ ...v, name: saveName(v) });
      setSaved(h);
      onSaved(h);
      next();
      void runTest(h.id);
    } catch (err) {
      setError(String(err));
    } finally {
      setSaving(false);
    }
  }

  return { saving, error, saved, test, runTest, saveAndTest };
}

export type HostSetup = ReturnType<typeof useHostSetup>;

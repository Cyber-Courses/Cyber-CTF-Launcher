import { useState } from "react";
import { serverSave, type CloudProvider, type RemoteProvider, type ServerHost, type ServerHostInput, type ServerTest, type SystemReport } from "@/lib/tauri";
import {
  connectionReady,
  keepsSecret,
  isCloud,
  machineTools,
  providerTraits,
  saveName,
  setupSteps,
  setupTitle,
  withProvider,
} from "@/features/servers/host-setup/setup-model";
import { useCloudAccounts } from "@/features/servers/host-setup/use-cloud-accounts";
import { useRunLog } from "@/features/servers/host-setup/use-run-log";
import { testHost } from "@/features/servers/use-server-list";
import { translate, useT } from "@/lib/i18n";
import { describeError } from "@/lib/errors";
import { useStepper } from "@/lib/use-stepper";

/** The setup's inputs: the host or account it starts from and the machine's tools, and what it
 *  tells its window. */
type SetupInput = {
  initial: ServerHostInput;
  report: SystemReport | null;
  onRefresh: () => void;
  /** After every save, so the main window's host list can refresh. */
  onSaved: (h: ServerHost) => void;
  /** Setup finished or cancelled: the window closes. */
  onDone: () => void;
};

/** All the server / cloud setup state and actions, shared by the setup steps. */
export function useHostSetup({ initial, report, onRefresh, onSaved, onDone }: SetupInput) {
  const t = useT();
  const form = useHostForm(initial);
  const { v } = form;
  const editing = initial.id !== null;
  const traits = providerTraits(v.provider);
  // Derived from the saved provider (not separate state) so it stays correct when editing an
  // existing account, where the provider step that would set it is skipped.
  const cloudProvider: CloudProvider = isCloud(v.provider) ? v.provider : "aws";
  const accounts = useCloudAccounts(v, form.setV, editing, cloudProvider);
  const stepper = useSteps(v.provider, editing);
  const tools = useToolInstall(onRefresh);
  const saving = useSaveAndTest(v, onSaved, stepper.next);
  const title = setupTitle({ saved: saving.saved, test: saving.test, editing, cloud: traits.cloud, initialName: initial.name });

  return {
    report,
    onRefresh,
    onDone,
    ...form,
    editing,
    cloudProvider,
    ...traits,
    ...accounts,
    ...stepper,
    connectionOk: connectionReady(v, keepsSecret(initial, v)),
    secretKept: keepsSecret(initial, v),
    ...tools,
    ...machineTools(v.provider, report),
    ...saving,
    title: t(`servers.setup.titles.${title.key}`, { name: title.name }),
  };
}

/** The form's values, and the helpers its fields and the provider tiles change them with. */
function useHostForm(initial: ServerHostInput) {
  const [v, setV] = useState<ServerHostInput>(initial);
  const set = <K extends keyof ServerHostInput>(k: K, value: ServerHostInput[K]) => {
    setV((s) => ({ ...s, [k]: value }));
  };
  const text = (k: "name" | "host" | "username" | "datastore" | "network" | "node") => ({
    value: v[k] ?? "",
    onChange: (e: React.ChangeEvent<HTMLInputElement>) => {
      set(k, e.target.value);
    },
  });
  const pickProvider = (id: CloudProvider) => {
    setV((s) => withProvider(s, id));
  };
  return { v, setV, set, text, pickProvider };
}

/** The steps this setup takes, and where the player is among them. */
function useSteps(provider: RemoteProvider, editing: boolean) {
  const steps = setupSteps(provider, editing);
  const { pos, next, back } = useStepper(steps.length);
  return { i: pos, steps, key: steps[pos], next, back };
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
    setToolLabel(label);
    setToolBusy(true);
    // The log says how it went: the run never rejects.
    await toolLog.run(translate("servers.setup.tools.installing", { name: label }), async (onLog) => {
      await run(onLog);
      return translate("servers.setup.tools.installDone");
    });
    setToolBusy(false);
    onRefresh();
  }
  return {
    pluginLog: toolLog.lines,
    toolLabel,
    toolBusy,
    installTool: (label: string, run: (onLog: (l: string) => void) => Promise<void>) => {
      if (!toolBusy) void installTool(label, run);
    },
  };
}

/** Saving the host, then moving to the Test step (`next`) and testing the saved host. */
function useSaveAndTest(v: ServerHostInput, onSaved: (h: ServerHost) => void, next: () => void) {
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [saved, setSaved] = useState<ServerHost | null>(null);
  const [test, setTest] = useState<ServerTest | "testing" | null>(null);

  // A failed test reads as one (`testHost` never rejects).
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
      setError(describeError(err));
    } finally {
      setSaving(false);
    }
  }

  return {
    saving,
    error,
    saved,
    test,
    runTest: (id: string) => {
      void runTest(id);
    },
    saveAndTest: () => {
      void saveAndTest();
    },
  };
}

export type HostSetup = ReturnType<typeof useHostSetup>;

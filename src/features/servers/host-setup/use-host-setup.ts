import { useEffect, useState } from "react";
import {
  serverDefaultSshKey,
  serverSave,
  serverSaveSshKey,
  type CloudProvider,
  type RemoteProvider,
  type ServerHost,
  type ServerHostInput,
  type ServerTest,
  type SshKeySignIn,
  type SystemReport,
} from "@/lib/tauri";
import {
  connectionReady,
  initialSignIn,
  initialSshKey,
  keepsSecret,
  makesToken,
  sameSshKey,
  savePayload,
  serverReady,
  type SignIn,
  isCloud,
  machineTools,
  providerTraits,
  setupSteps,
  setupTitle,
  withProvider,
} from "@/features/servers/host-setup/setup-model";
import { useCloudAccounts } from "@/features/servers/host-setup/use-cloud-accounts";
import { useRunLog } from "@/lib/use-run-log";
import { testHost } from "@/features/servers/use-server-list";
import { translate, useT } from "@/lib/i18n";
import { describeError } from "@/lib/errors";
import { useStepper } from "@/lib/use-stepper";
import { ignore } from "@/lib/failure";
import { useToolInstallsOff } from "@/lib/use-managed";

/** The setup's inputs: the host or account it starts from and the machine's tools, and what it
 *  tells its window. */
type SetupInput = {
  initial: ServerHostInput;
  /** A new cloud account opened on its provider already chosen: the setup skips choosing it. */
  providerChosen: boolean;
  report: SystemReport | null;
  onRefresh: () => void;
  /** After every save, so the main window's host list can refresh. */
  onSaved: (h: ServerHost) => void;
  /** Setup finished or cancelled: the window closes. */
  onDone: () => void;
};

/** All the server / cloud setup state and actions, shared by the setup steps. */
export function useHostSetup({ initial, providerChosen, report, onRefresh, onSaved, onDone }: SetupInput) {
  const t = useT();
  const form = useHostForm(initial);
  const { v } = form;
  const editing = initial.id !== null;
  const traits = providerTraits(v.provider);
  // Derived from the saved provider (not separate state) so it stays correct when editing an
  // existing account, where the provider step that would set it is skipped.
  const cloudProvider: CloudProvider = isCloud(v.provider) ? v.provider : "aws";
  const accounts = useCloudAccounts(v, form.setV, editing, cloudProvider);
  const stepper = useSteps(v.provider, editing, providerChosen);
  const tools = useToolInstall(onRefresh);
  const signIn = useSignIn(initial);
  const made = sameSshKey(signIn.madeWith, signIn.sshKey) && v.username.includes("!");
  const payload = savePayload(v, signIn.signInMethod, signIn.sshKey, made);
  const saving = useSaveAndTest(payload, onSaved, stepper.next);
  const connecting = useConnect(payload, form.setV, signIn.setMadeWith, onSaved, stepper.next);
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
    ...signIn,
    ...connecting,
    payload,
    serverOk: serverReady(v),
    connectionOk: connectionReady(v, keepsSecret(initial, v), v.provider === "proxmox" && signIn.signInMethod === "sshKey" ? signIn.sshKey : null),
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
  const text = (k: "name" | "host" | "username" | "datastore" | "network" | "node" | "imageStorage" | "snippetStorage") => ({
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

/** How a Proxmox setup signs in, and its SSH key sign-in (the first usual key offered when none
 *  is picked yet). */
function useSignIn(initial: ServerHostInput) {
  const [signInMethod, setSignInMethod] = useState<SignIn>(() => initialSignIn(initial));
  const [sshKey, setSshKeyState] = useState<SshKeySignIn>(() => initialSshKey(initial));
  const [touched, setTouched] = useState(false);
  // The key the saved token was made with (a saved SSH-key setup's, or the one just used).
  const [madeWith, setMadeWith] = useState<SshKeySignIn | null>(initial.sshKey ?? null);
  useEffect(() => {
    if (touched || initial.sshKey) return;
    serverDefaultSshKey()
      .then((path) => {
        if (path) setSshKeyState((k) => (k.path === "" ? { ...k, path } : k));
      })
      .catch(ignore("no default SSH key: the player picks one"));
  }, [touched, initial.sshKey]);
  const setSshKey = <K extends keyof SshKeySignIn>(k: K, value: SshKeySignIn[K]) => {
    setTouched(true);
    setSshKeyState((s) => ({ ...s, [k]: value }));
  };
  return { signInMethod, setSignInMethod, sshKey, setSshKey, madeWith, setMadeWith };
}

/** Leaving the Connection step. An SSH key sign-in whose token isn't made yet makes it there
 *  (`server_save_ssh_key`, which saves the host as a token setup), so Placement can read the
 *  server's options with it; the form then edits that saved host. */
function useConnect(
  payload: ServerHostInput,
  setV: React.Dispatch<React.SetStateAction<ServerHostInput>>,
  setMadeWith: (k: SshKeySignIn | null) => void,
  onSaved: (h: ServerHost) => void,
  next: () => void,
) {
  const [connecting, setConnecting] = useState(false);
  const [connectError, setConnectError] = useState<string | null>(null);
  async function connect() {
    if (!makesToken(payload)) {
      next();
      return;
    }
    setConnecting(true);
    setConnectError(null);
    try {
      const h = await serverSaveSshKey(payload);
      setV((s) => ({ ...s, id: h.id, username: h.username, password: null }));
      setMadeWith(h.sshKey ?? payload.sshKey ?? null);
      onSaved(h);
      next();
    } catch (err) {
      setConnectError(describeError(err));
    } finally {
      setConnecting(false);
    }
  }
  return {
    connecting,
    connectError,
    connect: () => {
      if (!connecting) void connect();
    },
  };
}

/** The steps this setup takes, and where the player is among them. */
function useSteps(provider: RemoteProvider, editing: boolean, providerChosen: boolean) {
  const steps = setupSteps(provider, editing, providerChosen);
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
  const installsOff = useToolInstallsOff();
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
    installsOff,
    /** No install runs, and the policy allows one. */
    canInstallTools: !toolBusy && !installsOff,
    installTool: (label: string, run: (onLog: (l: string) => void) => Promise<void>) => {
      if (!toolBusy && !installsOff) void installTool(label, run);
    },
  };
}

/** Saving the host (`v`, as `savePayload` builds it: with an SSH key sign-in, through the node
 *  bootstrap), then moving to the Test step (`next`) and testing the saved host. */
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
      const h = await (makesToken(v) ? serverSaveSshKey(v) : serverSave(v));
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

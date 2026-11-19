import type { Change, DockerEngine, PortMode, Provider, TestRecord, VmForm } from "@/lib/tauri";
import { IMAGE_PATTERN } from "@/lib/tauri/generated/Patterns";
import { translate } from "@/lib/i18n";
import { changeSettings, settingsNow } from "@/lib/settings-store";
import defaultAttackBox from "../../src-tauri/attack-box.json";

/** The player's settings, as the core keeps them (src-tauri/src/settings.rs), read from this
 *  window's copy (`settings-store`). */

/**
 * Default attack box: the Cyber CTF image (Kali with its standard toolset, published to
 * Docker Hub by the attack box repository, CyberCTF/attack-box), pinned by digest so every player on one launcher
 * version gets the same tested box. The digest lives in `src-tauri/attack-box.json`
 * (the Rust core reads it too), which the workflow's bump PR rewrites after each smoke-tested
 * publish. Exegol is licensed (free for personal use, paid for pro/commercial), so it's only
 * an opt-in preset.
 */
export const DEFAULT_ATTACK_IMAGE: string = defaultAttackBox.image;

/** The Cyber CTF image's repository, without the digest (what the settings show). */
export const CYBERCTF_ATTACK_REPO = "cyberctf/attack-box";

/** Common attack-box images; the field is free text so any tag/registry still works. Notes and
 *  terms are getters, read in the current language. `terms`: a short usage condition shown as a
 *  badge (e.g. Exegol's plan requirements). `large`: shown as "Large download" only when Docker
 *  Hub gives no size for the image. */
export const ATTACK_PRESETS: { image: string; label: string; note: string; large?: boolean; terms?: string }[] = [
  {
    image: DEFAULT_ATTACK_IMAGE,
    label: "Cyber CTF",
    get note() {
      return translate("common.attackPresets.cyberctf");
    },
    large: true,
  },
  {
    image: "kalilinux/kali-rolling",
    label: "Kali",
    get note() {
      return translate("common.attackPresets.kali");
    },
  },
  {
    image: "parrotsec/security",
    label: "Parrot",
    get note() {
      return translate("common.attackPresets.parrot");
    },
  },
  {
    image: "nwodtuhs/exegol:free",
    label: "Exegol Free",
    get note() {
      return translate("common.attackPresets.exegolFree");
    },
    get terms() {
      return translate("common.attackPresets.exegolFreeTerms");
    },
  },
  {
    image: "nwodtuhs/exegol:full",
    label: "Exegol Full",
    get note() {
      return translate("common.attackPresets.exegolFull");
    },
    large: true,
    get terms() {
      return translate("common.attackPresets.exegolFullTerms");
    },
  },
];

/** The attack box image the learner saved, null when they follow the default (the backend then
 *  runs its own, read from the same `src-tauri/attack-box.json`). The core keeps no earlier
 *  default: one saved before reads as null. */
export const savedAttackImage = (): string | null => settingsNow().attackImage;

/** The attack box image to use, as the settings show it: the learner's own, else the default. */
export const getAttackImage = (): string => savedAttackImage() ?? DEFAULT_ATTACK_IMAGE;

/** Saves the attack box image. The core keeps neither the default nor blank, so a later
 *  launcher's newer pinned default applies. */
export function setAttackImage(image: string) {
  changeSettings({ setting: "attackImage", value: image.trim() === DEFAULT_ATTACK_IMAGE ? null : image });
}

/** How an attack box image reads in the UI: the pinned default as its repository (the digest
 *  is noise to a player), anything else as saved. */
export function attackImageLabel(image: string): string {
  return image === DEFAULT_ATTACK_IMAGE ? CYBERCTF_ATTACK_REPO : image;
}

/** Defaults this launcher once had (the repository, its floating tag, each earlier pinned digest),
 *  for cleaning up what an older launcher downloaded. */
export const OLD_ATTACK_IMAGES: readonly string[] = [CYBERCTF_ATTACK_REPO, `${CYBERCTF_ATTACK_REPO}:latest`, ...defaultAttackBox.previous];

/** The hypervisor VM labs (and the VM test) run on, when several are installed. Null = automatic. */
export const getVmProvider = (): Provider | null => settingsNow().vmProvider;

/** Saves the hypervisor; every window follows (`useVmProvider`). */
export function setVmProvider(provider: Provider | null) {
  changeSettings({ setting: "vmProvider", value: provider });
}

/** An image name the core will run: the core's own pattern (`IMAGE_PATTERN`), plain ASCII name
 *  characters, no leading dash (it would read as a `docker` option). */
export const validImage = (image: string): boolean => IMAGE_PATTERN.test(image);

/** The last setup self-test result, per kind and per provider (an engine, a hypervisor), so the
 *  Machine page can show "tested 2 days ago" on each row; a failed one keeps why. */
export type LastTest = Omit<TestRecord, "why"> & { why?: string | null };

/** The key of a kind's record, or of one provider's. */
const testKey = (kind: "docker" | "vm", provider?: string | null) => (provider ? `${kind}.${provider}` : kind);

/** The last test of `kind` (the one labs run on), or of one `provider` of that kind. */
export const getLastTest = (kind: "docker" | "vm", provider?: string | null): LastTest | null => settingsNow().selfTests[testKey(kind, provider)] ?? null;

/** Records a test: under its `provider` when one is named, and as the kind's own unless
 *  `primary` is false (a test of an engine or hypervisor labs don't run on says nothing about
 *  the lab type's readiness). */
export function setLastTest(
  kind: "docker" | "vm",
  result: "ok" | "fail",
  { provider = null, primary = true, why = null }: { provider?: string | null; primary?: boolean; why?: string | null } = {},
) {
  const record: TestRecord = { result, at: Date.now(), why: result === "fail" ? why : null };
  const keys = [...(provider ? [testKey(kind, provider)] : []), ...(primary ? [testKey(kind)] : [])];
  if (keys.length > 0) changeSettings(...keys.map((key): Change => ({ setting: "selfTest", value: { key, record } })));
}

/** Start the attack box as soon as a lab the launcher reaches a run of is up, wherever it runs
 *  (on by default). */
export const getAutoAttackBox = (): boolean => settingsNow().attackAuto;

export const setAutoAttackBox = (on: boolean) => {
  changeSettings({ setting: "attackAuto", value: on });
};

/** Whether a VM attack box gets Kali's XFCE desktop (its Screen shows it, the player signed in),
 *  else stays headless (the default: a shell is enough, and the desktop is a larger set-up). A
 *  container box is always headless. */
export const getAttackDesktop = (): boolean => settingsNow().attackDesktop;

export const setAttackDesktop = (on: boolean) => {
  changeSettings({ setting: "attackDesktop", value: on });
};

/**
 * Host ports for a container lab on this machine: `random` free ports (never collide), or each
 * service on its `default` container port (CTFd on 8000), which fails the start when something
 * already holds one. Null: ask at each start.
 */
export type { PortMode };

export const getPortMode = (): PortMode | null => settingsNow().portMode;

export const setPortMode = (mode: PortMode | null) => {
  changeSettings({ setting: "portMode", value: mode });
};

/** The container engine a lab here runs on, saved by "don't ask again". Null: follow the current
 *  Docker context (what the CLI uses). */
export const getDockerEngine = (): DockerEngine | null => settingsNow().dockerEngine;

export const setDockerEngine = (engine: DockerEngine | null) => {
  changeSettings({ setting: "dockerEngine", value: engine });
};

/** For a dual-form lab here, the VM form saved by "don't ask again": `vms` = one VM per machine.
 *  Null: ask (the form step). */
export type SavedVmForm = VmForm;

export const getLocalVmForm = (): SavedVmForm | null => settingsNow().vmForm;

export const setLocalVmForm = (form: SavedVmForm | null) => {
  changeSettings({ setting: "vmForm", value: form });
};

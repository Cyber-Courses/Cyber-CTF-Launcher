import { DOCKER_ENGINES, LOCAL_PROVIDERS, type DockerEngine, type Provider } from "@/lib/tauri";
import { translate } from "@/lib/i18n";
import { readStored, readStoredOneOf, writeStored } from "@/lib/local-store";
import { ignore } from "@/lib/failure";
import defaultAttackBox from "../../src-tauri/attack-box-image.json";

/** Per-machine launcher preferences, kept locally. */

const ATTACK_IMAGE_KEY = "cyberctf.attackbox.image";

/**
 * Default attack box: the Cyber CTF image (Kali with its standard toolset, published to
 * Docker Hub by the attack-box workflow), pinned by digest so every player on one launcher
 * version gets the same tested box. The digest lives in `src-tauri/attack-box-image.json`
 * (the Rust core reads it too), which the workflow's bump PR rewrites after each smoke-tested
 * publish. Exegol is licensed (free for personal use, paid for pro/commercial), so it's only
 * an opt-in preset.
 */
export const DEFAULT_ATTACK_IMAGE: string = defaultAttackBox.image;

/** The Cyber CTF image's repository, without the digest (what the settings show). */
export const CYBERCTF_ATTACK_REPO = "cyberctf/attack-box";

/** Defaults this launcher once had: the floating tag, then each earlier pinned digest. A saved
 *  image equal to one of them follows the current default; any other saved image is the
 *  player's own and stays. */
const OLD_DEFAULT_IMAGES = new Set<string>([CYBERCTF_ATTACK_REPO, `${CYBERCTF_ATTACK_REPO}:latest`, ...defaultAttackBox.previous]);

/** Common attack-box images; the field is free text so any tag/registry still works. Notes and
 *  terms are getters, read in the current language. */
/** `terms`: a short usage condition shown as a badge (e.g. Exegol's plan requirements).
 *  `large`: shown as "Large download" only when Docker Hub gives no size for the image. */
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
 *  runs its own, read from the same `attack-box-image.json`). A saved old default is migrated:
 *  the key is cleared, so this machine follows the default from then on. */
export function savedAttackImage(): string | null {
  const saved = readStored(ATTACK_IMAGE_KEY);
  if (saved && OLD_DEFAULT_IMAGES.has(saved)) writeStored(ATTACK_IMAGE_KEY, null);
  else if (saved) return saved;
  return null;
}

/** The attack box image to use, as the settings show it: the learner's own, else the default. */
export const getAttackImage = (): string => savedAttackImage() ?? DEFAULT_ATTACK_IMAGE;

/** Saves the attack box image. The default (or blank) isn't stored, so a later launcher's newer
 *  pinned default applies without another migration. */
export function setAttackImage(image: string) {
  const value = image.trim();
  writeStored(ATTACK_IMAGE_KEY, !value || value === DEFAULT_ATTACK_IMAGE || OLD_DEFAULT_IMAGES.has(value) ? null : value);
}

/** How an attack box image reads in the UI: the pinned default as its repository (the digest
 *  is noise to a player), anything else as saved. */
export function attackImageLabel(image: string): string {
  return image === DEFAULT_ATTACK_IMAGE ? CYBERCTF_ATTACK_REPO : image;
}

/** Earlier default images, for cleaning up what an older launcher downloaded. */
export const OLD_ATTACK_IMAGES: readonly string[] = [...OLD_DEFAULT_IMAGES];

const VM_PROVIDER_KEY = "cyberctf.vm.provider";

/** Sent on `window` when the hypervisor choice changes (Settings, the Machine setup, or "Don't
 *  ask again" in "Where should it run?"), so the header and the lab page follow at once. */
export const VM_PROVIDER_CHANGED = "cyberctf:vm-provider";

/** The hypervisor VM labs (and the VM test) run on, when several are installed. Null = automatic
 *  (also for a saved value that names no hypervisor of this machine's kind). */
export const getVmProvider = (): Provider | null => readStoredOneOf(VM_PROVIDER_KEY, LOCAL_PROVIDERS);

export function setVmProvider(provider: Provider | null) {
  writeStored(VM_PROVIDER_KEY, provider);
  if (typeof window !== "undefined") window.dispatchEvent(new Event(VM_PROVIDER_CHANGED));
}

/** An image name the core will run (`valid_image` in src-tauri/src/platform/exec/docker.rs): plain ASCII name characters, no
 *  leading dash (it would read as a `docker` option). */
export function validImage(image: string): boolean {
  return image.length > 0 && image.length <= 200 && !image.startsWith("-") && /^[A-Za-z0-9._\-/:@]+$/.test(image);
}

const TEST_KEY = "cyberctf.selftest.";

/** The last setup self-test result, per kind and per provider (an engine, a hypervisor), so the
 *  Machine page can show "tested 2 days ago" on each row; a failed one keeps why. */
export interface LastTest {
  result: "ok" | "fail";
  at: number;
  /** The failing step's detail, for a failed test. */
  why?: string | null;
}

/** The key of a kind's record, or of one provider's. */
const testKey = (kind: "docker" | "vm", provider?: string | null) => (provider ? `${TEST_KEY}${kind}.${provider}` : TEST_KEY + kind);

/** The last test of `kind` (the one labs run on), or of one `provider` of that kind. */
export function getLastTest(kind: "docker" | "vm", provider?: string | null): LastTest | null {
  const raw = readStored(testKey(kind, provider));
  const v = raw ? parsedRecord(raw) : null;
  // Only a record of the right shape: anything else (an older format, a hand edit) is none.
  const ok = !!v && typeof v === "object" && ["ok", "fail"].includes((v as LastTest).result) && Number.isFinite((v as LastTest).at);
  return ok ? (v as LastTest) : null;
}

/** A saved record's JSON; null when it isn't JSON (a hand edit). */
function parsedRecord(raw: string): unknown {
  try {
    return JSON.parse(raw);
  } catch (e) {
    return ignore("a saved test record that isn't JSON reads as none", () => null)(e);
  }
}

/** Records a test: under its `provider` when one is named, and as the kind's own unless
 *  `primary` is false (a test of an engine or hypervisor labs don't run on says nothing about
 *  the lab type's readiness). */
export function setLastTest(
  kind: "docker" | "vm",
  result: "ok" | "fail",
  { provider = null, primary = true, why = null }: { provider?: string | null; primary?: boolean; why?: string | null } = {},
) {
  const record = JSON.stringify({ result, at: Date.now(), why: result === "fail" ? why : null });
  if (provider) writeStored(testKey(kind, provider), record);
  if (primary) writeStored(testKey(kind), record);
}

const AUTO_ATTACK_KEY = "cyberctf.attackbox.auto";

/** Start the attack box as soon as a lab the launcher reaches a run of is up, wherever it runs
 *  (default on). */
export const getAutoAttackBox = (): boolean => readStored(AUTO_ATTACK_KEY) !== "false";

export const setAutoAttackBox = (on: boolean) => {
  writeStored(AUTO_ATTACK_KEY, on ? "true" : "false");
};

const PORT_MODE_KEY = "cyberctf.labs.ports";

/**
 * Host ports for a container lab on this machine: `random` free ports (never collide), or each
 * service on its `default` container port (CTFd on 8000), which fails the start when something
 * already holds one. Null: ask at each start.
 */
export type PortMode = "random" | "default";

export const getPortMode = (): PortMode | null => readStoredOneOf(PORT_MODE_KEY, ["random", "default"]);

export const setPortMode = (mode: PortMode | null) => {
  writeStored(PORT_MODE_KEY, mode);
};

const DOCKER_ENGINE_KEY = "cyberctf.docker.engine";
const LOCAL_VM_FORM_KEY = "cyberctf.labs.vmForm";

/** The container engine a lab here runs on, saved by "don't ask again". Null: follow the current
 *  Docker context (what the CLI uses), also for a saved value that names no engine. */
export const getDockerEngine = (): DockerEngine | null => readStoredOneOf(DOCKER_ENGINE_KEY, DOCKER_ENGINES);

export const setDockerEngine = (engine: DockerEngine | null) => {
  writeStored(DOCKER_ENGINE_KEY, engine);
};

/** For a dual-form lab here, the VM form saved by "don't ask again": `vm` = one helper VM,
 *  `vms` = one VM per machine. Null: ask which (the form step). */
export type LocalVmForm = "vm" | "vms";

export const getLocalVmForm = (): LocalVmForm | null => readStoredOneOf(LOCAL_VM_FORM_KEY, ["vm", "vms"]);

export const setLocalVmForm = (form: LocalVmForm | null) => {
  writeStored(LOCAL_VM_FORM_KEY, form);
};

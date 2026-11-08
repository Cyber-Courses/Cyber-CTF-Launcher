import type { DockerEngine, Provider } from "@/lib/tauri";
import { translate } from "@/lib/i18n";
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

/** The attack box image to use. A saved old default is migrated: the key is cleared, so this
 *  machine follows the default from then on. */
export function getAttackImage(): string {
  try {
    const saved = localStorage.getItem(ATTACK_IMAGE_KEY);
    if (saved && OLD_DEFAULT_IMAGES.has(saved)) localStorage.removeItem(ATTACK_IMAGE_KEY);
    else if (saved) return saved;
    return DEFAULT_ATTACK_IMAGE;
  } catch {
    return DEFAULT_ATTACK_IMAGE;
  }
}

/** Saves the attack box image. The default (or blank) isn't stored, so a later launcher's newer
 *  pinned default applies without another migration. */
export function setAttackImage(image: string) {
  try {
    const value = image.trim();
    if (!value || value === DEFAULT_ATTACK_IMAGE || OLD_DEFAULT_IMAGES.has(value)) localStorage.removeItem(ATTACK_IMAGE_KEY);
    else localStorage.setItem(ATTACK_IMAGE_KEY, value);
  } catch {
    /* ignore */
  }
}

/** How an attack box image reads in the UI: the pinned default as its repository (the digest
 *  is noise to a player), anything else as saved. */
export function attackImageLabel(image: string): string {
  return image === DEFAULT_ATTACK_IMAGE ? CYBERCTF_ATTACK_REPO : image;
}

/** Earlier default images, for cleaning up what an older launcher downloaded. */
export const OLD_ATTACK_IMAGES: readonly string[] = [...OLD_DEFAULT_IMAGES];

const ATTACK_BOX_KEY = "cyberctf.attackbox.box";

/** Default attack VM beside VM labs: the official Kali Vagrant box (VirtualBox, VMware, Hyper-V). */
export const DEFAULT_ATTACK_BOX = "kalilinux/rolling";

/** Vagrant boxes for the attack VM; the field is free text so any `owner/name` box works. */
export const ATTACK_VM_PRESETS: { box: string; label: string; note: string }[] = [
  {
    box: "kalilinux/rolling",
    label: "Kali",
    get note() {
      return translate("common.attackPresets.kaliVm");
    },
  },
  {
    box: "generic/debian12",
    label: "Debian 12",
    get note() {
      return translate("common.attackPresets.debianVm");
    },
  },
];

export function getAttackBox(): string {
  return getAttackBoxChoice() ?? DEFAULT_ATTACK_BOX;
}

/** The attack VM box the learner picked in Settings, or null when they never did: the lab's own
 *  default then runs, the box its Vagrantfile picks for this machine's CPU (Kali on x86, the
 *  Debian cloud image on Arm, where Kali's box has no build). */
export function getAttackBoxChoice(): string | null {
  try {
    return localStorage.getItem(ATTACK_BOX_KEY) || null;
  } catch {
    return null;
  }
}

export function setAttackBox(box: string) {
  try {
    localStorage.setItem(ATTACK_BOX_KEY, box.trim() || DEFAULT_ATTACK_BOX);
  } catch {
    /* ignore */
  }
}

const VM_PROVIDER_KEY = "cyberctf.vm.provider";

/** Sent on `window` when the hypervisor choice changes (Settings, the Machine setup, or the
 *  switch in "Where should it run?"), so the header and the lab page follow at once. */
export const VM_PROVIDER_CHANGED = "cyberctf:vm-provider";

/** The hypervisor VM labs (and the VM test) run on, when several are installed. Null = automatic. */
export function getVmProvider(): Provider | null {
  try {
    return (localStorage.getItem(VM_PROVIDER_KEY) as Provider | null) || null;
  } catch {
    return null;
  }
}

export function setVmProvider(provider: Provider | null) {
  try {
    if (provider) localStorage.setItem(VM_PROVIDER_KEY, provider);
    else localStorage.removeItem(VM_PROVIDER_KEY);
  } catch {
    /* ignore */
  }
  if (typeof window !== "undefined") window.dispatchEvent(new Event(VM_PROVIDER_CHANGED));
}

/** An image name the core will run (exegol.rs `valid_image`): plain ASCII name characters, no
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
  try {
    const raw = localStorage.getItem(testKey(kind, provider));
    const v: unknown = raw ? JSON.parse(raw) : null;
    // Only a record of the right shape: anything else (an older format, a hand edit) is none.
    const ok = !!v && typeof v === "object" && ["ok", "fail"].includes((v as LastTest).result) && Number.isFinite((v as LastTest).at);
    return ok ? (v as LastTest) : null;
  } catch {
    return null;
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
  try {
    const record = JSON.stringify({ result, at: Date.now(), why: result === "fail" ? why : null });
    if (provider) localStorage.setItem(testKey(kind, provider), record);
    if (primary) localStorage.setItem(testKey(kind), record);
  } catch {
    /* ignore */
  }
}

const AUTO_ATTACK_KEY = "cyberctf.attackbox.auto";

/** Start the attack box as soon as a local container lab is up (default on). */
export function getAutoAttackBox(): boolean {
  try {
    return localStorage.getItem(AUTO_ATTACK_KEY) !== "false";
  } catch {
    return true;
  }
}

export function setAutoAttackBox(on: boolean) {
  try {
    localStorage.setItem(AUTO_ATTACK_KEY, on ? "true" : "false");
  } catch {
    /* ignore */
  }
}

const PORT_MODE_KEY = "cyberctf.labs.ports";

/**
 * Host ports for a container lab on this machine: `random` free ports (never collide), or each
 * service on its `default` container port (CTFd on 8000), which fails the start when something
 * already holds one. Null: ask at each start.
 */
export type PortMode = "random" | "default";

export function getPortMode(): PortMode | null {
  try {
    const v = localStorage.getItem(PORT_MODE_KEY);
    return v === "random" || v === "default" ? v : null;
  } catch {
    return null;
  }
}

export function setPortMode(mode: PortMode | null) {
  try {
    if (mode) localStorage.setItem(PORT_MODE_KEY, mode);
    else localStorage.removeItem(PORT_MODE_KEY);
  } catch {
    /* ignore */
  }
}

const DOCKER_ENGINE_KEY = "cyberctf.docker.engine";
const LOCAL_VM_FORM_KEY = "cyberctf.labs.vmForm";

/** Sent on `window` when a saved launch choice (engine, ports, VM form, hypervisor) changes, so
 *  Settings and the lab page follow at once. */
const LAUNCH_CHOICE_CHANGED = "cyberctf:launch-choice";

function emitLaunchChoice() {
  if (typeof window !== "undefined") window.dispatchEvent(new Event(LAUNCH_CHOICE_CHANGED));
}

/** The container engine a lab here runs on, saved by "don't ask again". Null: follow the current
 *  Docker context (what the CLI uses). */
export function getDockerEngine(): DockerEngine | null {
  try {
    return (localStorage.getItem(DOCKER_ENGINE_KEY) as DockerEngine | null) || null;
  } catch {
    return null;
  }
}

export function setDockerEngine(engine: DockerEngine | null) {
  try {
    if (engine) localStorage.setItem(DOCKER_ENGINE_KEY, engine);
    else localStorage.removeItem(DOCKER_ENGINE_KEY);
  } catch {
    /* ignore */
  }
  emitLaunchChoice();
}

/** For a dual-form lab here, the VM form saved by "don't ask again": `vm` = one helper VM,
 *  `vms` = one VM per machine. Null: ask which (the form step). */
export type LocalVmForm = "vm" | "vms";

export function getLocalVmForm(): LocalVmForm | null {
  try {
    const v = localStorage.getItem(LOCAL_VM_FORM_KEY);
    return v === "vm" || v === "vms" ? v : null;
  } catch {
    return null;
  }
}

export function setLocalVmForm(form: LocalVmForm | null) {
  try {
    if (form) localStorage.setItem(LOCAL_VM_FORM_KEY, form);
    else localStorage.removeItem(LOCAL_VM_FORM_KEY);
  } catch {
    /* ignore */
  }
  emitLaunchChoice();
}

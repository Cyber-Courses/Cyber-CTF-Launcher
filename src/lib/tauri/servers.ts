import { invoke } from "@tauri-apps/api/core";
import { invokeStreaming, invokeUnit } from "@/lib/tauri/invoke";
import type * as Generated from "@/lib/tauri/generated";
import type {
  HostCapacity,
  ProxmoxBridge,
  ProxmoxNodeChoices,
  ProxmoxPlacement,
  SelfTestEvent,
  ServerCheck,
  ServerCheckState,
  ServerTest,
  SshKeySignIn,
} from "@/lib/tauri/generated";
import type { CloudProvider } from "@/lib/tauri/cloud";
import { SERVER_PROVIDERS } from "@/lib/tauri/generated/ProviderNames";

export type { HostCapacity, ProxmoxBridge, ProxmoxNodeChoices, ProxmoxPlacement, ServerCheck, ServerCheckState, ServerTest, SshKeySignIn };

// --- Server (the player's own ESXi / Proxmox host) ---

/** The player's own servers' providers (the backend's `Provider::SERVERS`). */
export type ServerProvider = (typeof SERVER_PROVIDERS)[number];

/** A saved host's provider: one of the player's servers, or a cloud account. */
export type RemoteProvider = ServerProvider | CloudProvider;

// A host's provider is a `Provider` on the Rust side, which refuses a local one on save: only
// remote ones are ever stored or submitted.
export type ServerHost = Omit<Generated.ServerHost, "provider"> & { provider: RemoteProvider };
/** Form payload; `id` null creates a host, `password` null keeps the stored one. */
export type ServerHostInput = Omit<Generated.ServerHostInput, "provider"> & { provider: RemoteProvider };
export type ServerList = Omit<Generated.ServerList, "hosts"> & { hosts: ServerHost[] };

/** The player's own servers, as opposed to cloud accounts, in the launcher's order: the backend's
 *  `Provider::SERVERS`, each a host's provider. */
export { SERVER_PROVIDERS };

/** Each kind of server and cloud account (how it signs in, its default port, the cloud CLI it
 *  needs here), and an account's default auto-stop: the backend's. */
export { DEFAULT_AUTO_STOP_HOURS, HOST_KINDS } from "@/lib/tauri/generated/HostKinds";

/** Whether `provider` is one of the player's own servers', not a cloud account's. */
export const isServerProvider = (provider: string | null | undefined): provider is ServerProvider =>
  !!provider && (SERVER_PROVIDERS as readonly string[]).includes(provider);

export const serverList = () => invoke<ServerList>("server_list");
export const serverSave = (input: ServerHostInput) => invoke<ServerHost>("server_save", { input });
/** Saves a Proxmox host set up with the player's SSH key (`input.sshKey`): the launcher signs in
 *  to the node with it, makes its own API token there and authorizes its key, then saves the host
 *  as a token setup. Rejects, saving nothing, when any part fails. */
export const serverSaveSshKey = (input: ServerHostInput) => invoke<ServerHost>("server_save_ssh_key", { input });
/** A Proxmox server's nodes, storages and bridges, read from its API with the form's sign-in (its
 *  password or token secret, else the saved host's). Rejects when the API can't be read. */
export const serverProxmoxPlacement = (input: ServerHostInput) => invoke<ProxmoxPlacement>("server_proxmox_placement", { input });
/** The first of ~/.ssh/id_ed25519, id_ecdsa, id_rsa that exists (null when none does). */
export const serverDefaultSshKey = () => invoke<string | null>("server_default_ssh_key");
export const serverRemove = (id: string) => invokeUnit("server_remove", { id });
export const serverSetDefault = (id: string | null) => invokeUnit("server_set_default", { id });
export const serverTest = (id: string) => invoke<ServerTest>("server_test", { id });
/** The launcher's SSH public key; token-auth Proxmox hosts must authorize it for SSH. */
export const serverPublicKey = () => invoke<string>("server_public_key");
/** How many installed labs are currently running on each host, keyed by host id. */
export const serverRunningLabs = () => invoke<Record<string, number>>("server_running_labs");
/** This month's spend so far (USD) on a saved AWS account, with its own credentials: the figure
 *  the launch budget gate checks. Rejects when it can't be read. */
export const serverMonthToDateCost = (id: string) => invoke<number>("server_month_to_date_cost", { id });
/** A host's hardware headroom: null for a host that isn't a Proxmox server; rejects when it
 *  can't be read. */
export const serverCapacity = (id: string) => invoke<HostCapacity | null>("server_capacity", { id });
/** The steps each server's VM test reports, in order, by provider (generated from the core's). */
export { SELFTEST_STEPS } from "@/lib/tauri/generated/SelfTestSteps";
/** Real-VM self-test: provisions a throwaway VM on the host, checks it, then destroys it. */
export const serverSelftest = (id: string, onEvent: (e: SelfTestEvent) => void) => invokeStreaming("server_selftest", { id }, onEvent, "events");
/** Opens (or focuses) the setup window, for a new host or to edit `id`. A new cloud account's
 *  setup opens on `provider` when one is named (the Cloud page's Connect rows). */
export const serverOpenSetup = (id: string | null, kind: "server" | "cloud" = "server", provider: CloudProvider | null = null) =>
  invokeUnit("server_open_setup", provider ? { id, kind, provider } : { id, kind });
/** Emitted by the setup window after a save; the main window reloads its host list. */
export const SERVER_CHANGED = "server:changed";

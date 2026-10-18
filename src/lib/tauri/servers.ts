import { invoke } from "@tauri-apps/api/core";
import { invokeStreaming, invokeUnit } from "@/lib/tauri/invoke";
import type * as Generated from "@/lib/tauri/generated";
import type { HostCapacity, Provider, SelfTestEvent, ServerCheck, ServerCheckState, ServerTest } from "@/lib/tauri/generated";

export type { HostCapacity, ServerCheck, ServerCheckState, ServerTest };

// --- Server (the player's own ESXi / Proxmox host) ---

export type RemoteProvider = Extract<Provider, "vmware_esxi" | "proxmox" | "aws" | "azure" | "gcp" | "digitalocean" | "linode" | "oci">;

// A host's provider is a `Provider` on the Rust side, which refuses a local one on save: only
// remote ones are ever stored or submitted.
export type ServerHost = Omit<Generated.ServerHost, "provider"> & { provider: RemoteProvider };
/** Form payload; `id` null creates a host, `password` null keeps the stored one. */
export type ServerHostInput = Omit<Generated.ServerHostInput, "provider"> & { provider: RemoteProvider };
export type ServerList = Omit<Generated.ServerList, "hosts"> & { hosts: ServerHost[] };

export const serverList = () => invoke<ServerList>("server_list");
export const serverSave = (input: ServerHostInput) => invoke<ServerHost>("server_save", { input });
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
/** A host's hardware headroom (Proxmox only; null otherwise or on failure). */
export const serverCapacity = (id: string) => invoke<HostCapacity | null>("server_capacity", { id });
/** Real-VM self-test: provisions a throwaway VM on the host, checks it, then destroys it. */
export const serverSelftest = (id: string, onEvent: (e: SelfTestEvent) => void) => invokeStreaming("server_selftest", { id }, onEvent, "events");
/** Opens (or focuses) the setup window, for a new host or to edit `id`. */
export const serverOpenSetup = (id: string | null, kind: "server" | "cloud" = "server") => invokeUnit("server_open_setup", { id, kind });
/** Emitted by the setup window after a save; the main window reloads its host list. */
export const SERVER_CHANGED = "server:changed";

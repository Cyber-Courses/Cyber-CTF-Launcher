// placeholder until cargo regenerates it
import type { DockerEngine } from "./DockerEngine";
import type { Provider } from "./Provider";
export type LabPlacement = { "kind": "docker", engine: DockerEngine | null, defaultPorts: boolean, } | { "kind": "vms", provider: Provider, } | { "kind": "server", host: string, } | { "kind": "cloud", account: string, perMachine: boolean, };

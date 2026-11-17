// placeholder until cargo regenerates it
import type { DockerEngine } from "./DockerEngine";
import type { Provider } from "./Provider";
export type LabPlacementChoice = { here: boolean, host: string | null, provider: Provider | null, perMachineVm: boolean, engine: DockerEngine | null, defaultPorts: boolean | null, };

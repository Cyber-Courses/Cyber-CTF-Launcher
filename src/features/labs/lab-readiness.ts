import type { Lab } from "@/features/labs/use-labs";
import { cloudAccountsFor, cloudNames, localProviders } from "@/features/labs/lab-runtime";
import { providerLabel } from "@/features/machine/hypervisors";
import type { ServerHost, SystemReport } from "@/lib/tauri";
import { translate } from "@/lib/i18n";

/**
 * What this machine is missing to run a lab, from facts the launcher already checked: a
 * running container engine for Docker labs; for VM labs, a local hypervisor the lab
 * supports that is fully usable (hypervisor + Vagrant + plugin), or a saved server it
 * can run on; for cloud labs, Terraform and a connected account of a cloud the lab runs on
 * (its sign-in is checked when the account is picked). Null when it can run.
 */
export function setupNeeded(lab: Lab, report: SystemReport | null, servers: ServerHost[]): string | null {
  const rt = lab.runtime;
  if (!rt || !report) return null;
  if (rt.runtime === "CLOUD") return cloudSetupNeeded(lab, report, servers);
  if (rt.runtime === "DOCKER") {
    if (!report.docker.installed) return translate("labs.readiness.needsEngine");
    if (report.dockerDenied) return translate("labs.readiness.dockerPermission");
    if (!report.dockerRunning) return translate("labs.readiness.startEngine");
    return null;
  }
  // Built for another CPU, only an emulator (QEMU) runs it here.
  const here = localProviders(rt, report.arch);
  const local = report.vmProviders.some((p) => !p.remote && p.available && here.includes(p.provider));
  const remote = servers.some((h) => rt.providers.includes(h.provider));
  if (local || remote) return null;
  // Name the hypervisors this lab supports, not a generic "a hypervisor": a lab that runs on
  // VirtualBox or VMware says so, instead of looking as if nothing is installed.
  const supported = report.vmProviders.filter((p) => !p.remote && here.includes(p.provider)).map((p) => providerLabel(p, report.os));
  if (supported.length === 0) return translate("labs.readiness.needsServer");
  return translate("labs.readiness.needsHypervisor", { names: joinOr(supported) });
}

/** What a cloud lab is missing: an account of its cloud, then Terraform. */
function cloudSetupNeeded(lab: Lab, report: SystemReport, servers: ServerHost[]): string | null {
  if (cloudAccountsFor(lab.runtime, servers).length === 0)
    return translate("labs.readiness.needsCloudAccount", { clouds: joinOr(cloudNames(lab.runtime!.providers)) });
  if (!report.terraform.installed) return translate("labs.readiness.needsTerraform");
  return null;
}

/** Whether a setup message is a cloud lab's missing account (the row offers to connect one). */
export const needsCloudAccount = (lab: Lab, servers: ServerHost[]) => lab.runtime?.runtime === "CLOUD" && cloudAccountsFor(lab.runtime, servers).length === 0;

/** "A", "A or B", "A, B or C". */
export function joinOr(items: string[]): string {
  if (items.length <= 1) return items.join("");
  return translate("labs.readiness.or", { rest: items.slice(0, -1).join(", "), last: items[items.length - 1] });
}

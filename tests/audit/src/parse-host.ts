// The leftover audit's parsers of what any machine holds: Docker's objects, Vagrant's machines and
// boxes, the stack's folders and files, its processes and listening ports, the network's
// configuration and the credential store. A Windows probe (snapshot.ps1) of a kind that has no
// common tool writes JSON lines, which the same kind's parser reads too.

import type { Item, Parser } from "./inventory.ts";
import { epoch, jsonLines, lineItems, lines, pathItem, refs } from "./parse-util.ts";

type Labels = Record<string, string> | null | undefined;

/** The names a Docker object's labels tie it to: its Compose project and its Isoloom lab. */
const labelRefs = (labels: Labels): string[] => refs(labels?.["com.docker.compose.project"], labels?.["isoloom.environment"]);

/** The folders a container's Compose project was started from (its labels). */
function composePaths(labels: Labels): string[] {
  const files = labels?.["com.docker.compose.project.config_files"] ?? "";
  return refs(labels?.["com.docker.compose.project.working_dir"], ...files.split(","));
}

const str = (v: unknown): string => (typeof v === "string" ? v : typeof v === "number" ? String(v) : "");

const dockerContainer: Parser = (output) =>
  jsonLines(output).map((c) => {
    const labels = c.labels as Labels;
    const name = str(c.name).replace(/^\//, "");
    return { kind: "docker-container", key: str(c.id), name, refs: refs(name, ...labelRefs(labels)), created: epoch(c.created), paths: composePaths(labels) };
  });

const dockerNetwork: Parser = (output) =>
  jsonLines(output).map((n) => {
    const name = str(n.name);
    return { kind: "docker-network", key: str(n.id), name, refs: refs(name, ...labelRefs(n.labels as Labels)), created: epoch(n.created), paths: [] };
  });

/** A volume, its name marked ` (persistent)` when Isoloom keeps it on purpose (the label
 *  `isoloom.persistent`: `down` leaves it), which the allowlist reads. */
const dockerVolume: Parser = (output) =>
  jsonLines(output).map((v) => {
    const key = str(v.name);
    const labels = v.labels as Labels;
    const name = labels?.["isoloom.persistent"] === "true" ? `${key} (persistent)` : key;
    return { kind: "docker-volume", key, name, refs: refs(key, ...labelRefs(labels)), created: epoch(v.created), paths: [] };
  });

const dockerImage: Parser = (output) =>
  jsonLines(output).map((i) => {
    const repo = str(i.Repository);
    const name = `${repo}:${str(i.Tag)}`;
    return { kind: "docker-image", key: `${str(i.ID)} ${name}`, name, refs: refs(repo), created: epoch(i.CreatedAt), paths: [] };
  });

/** Vagrant's machine index (data/machine-index/index): every machine any Vagrantfile made. */
const vagrantMachine: Parser = (output) => {
  if (output.trim() === "") return [];
  const index = JSON.parse(output) as { machines?: Record<string, Record<string, unknown>> };
  return Object.entries(index.machines ?? {}).map(([id, m]) => {
    const dir = str(m.vagrantfile_path);
    const data = str(m.local_data_path);
    return {
      kind: "vagrant-machine",
      key: id,
      name: `${str(m.name)} (${str(m.provider)}, ${dir || data})`,
      refs: refs(str(m.name), dir, data),
      paths: refs(dir, data),
    };
  });
};

/** Vagrant's boxes, as their folders under boxes/: `<name>/<version>[/<arch>]/<provider>`. */
const vagrantBox: Parser = (output) =>
  lines(output).map((l) => {
    const parts = l.trim().split(/[\\/]/);
    const name = parts[0].replaceAll("-VAGRANTSLASH-", "/");
    const label = `${name} ${parts.slice(1).join(" ")}`;
    return { kind: "vagrant-box", key: l.trim(), name: label, refs: [name], paths: [] };
  });

/** `[[dd-]hh:]mm:ss`, ps's elapsed time, in seconds. */
export function elapsed(etime: string): number {
  const [days, rest] = etime.includes("-") ? etime.split("-") : ["0", etime];
  const parts = rest.split(":").map(Number);
  while (parts.length < 3) parts.unshift(0);
  return Number(days) * 86400 + parts[0] * 3600 + parts[1] * 60 + parts[2];
}

/** The names in a command line that tie it to a run: a Compose project, a lab's `cyberctf-…`. */
const commandRefs = (command: string): string[] => refs(...[...command.matchAll(/--project-name[ =](\S+)|\b(cyberctf-[\w.-]+)/g)].map((m) => m[1] || m[2]));

/** The absolute paths in a command line (a run's root, a lab's folder). */
const commandPaths = (command: string): string[] => refs(...[...command.matchAll(/(?:^|[\s="'])((?:\/|[A-Za-z]:\\)[^\s"']+)/g)].map((m) => m[1]));

/** The stack's processes: `ps`'s `<pid> <etime> <command>` lines and its `now <epoch>`, or a
 *  Windows probe's JSON (ProcessId, CommandLine, CreationDate). */
const processes: Parser = (output, taken) => {
  const json = jsonLines(output);
  if (json.length > 0) {
    return json.map((p) => {
      const command = str(p.CommandLine) || str(p.Name);
      const pid = Number(p.ProcessId);
      return {
        kind: "process",
        key: `${String(pid)} ${command}`,
        name: `${String(pid)} ${command}`,
        refs: commandRefs(command),
        created: epoch(p.CreationDate),
        paths: commandPaths(command),
        pid,
      };
    });
  }
  const now = Number(/^now (\d+)$/m.exec(output)?.[1] ?? taken);
  return lines(output).flatMap((l) => {
    const m = /^\s*(\d+)\s+(\S+)\s+(.*)$/.exec(l);
    if (!m || l.startsWith("now ")) return [];
    const pid = Number(m[1]);
    const command = m[3].trim();
    // The start, to the minute: two snapshots' clocks round differently.
    const created = Math.floor((now - elapsed(m[2])) / 60) * 60;
    return [{ kind: "process", key: `${m[1]} ${command}`, name: `${m[1]} ${command}`, refs: commandRefs(command), created, paths: commandPaths(command), pid }];
  });
};

/** A listening socket, by protocol, address and port: lsof's -F fields (p pid, c command, n
 *  address), ss's lines, or a Windows probe's JSON (LocalAddress, LocalPort, OwningProcess, Name). */
const listeningPorts: Parser = (output) => {
  const json = jsonLines(output);
  if (json.length > 0) {
    return json.map((p) => port(`${str(p.LocalAddress)}:${str(p.LocalPort)}`, str(p.Name), Number(p.OwningProcess)));
  }
  if (/^p\d+$/m.test(output)) {
    const out: Item[] = [];
    let pid = 0;
    let command = "";
    for (const l of lines(output)) {
      if (l.startsWith("p")) pid = Number(l.slice(1));
      else if (l.startsWith("c")) command = l.slice(1);
      else if (l.startsWith("n")) out.push(port(l.slice(1), command, pid));
    }
    return dedupe(out);
  }
  return lines(output).flatMap((l) => {
    const f = l.trim().split(/\s+/);
    if (f.length < 5) return [];
    const proc = /\("([^"]+)",pid=(\d+)/.exec(l);
    return [port(`${f[0]} ${f[4]}`, proc?.[1] ?? "", Number(proc?.[2] ?? 0))];
  });
};

const port = (address: string, command: string, pid: number): Item => ({
  kind: "listening-port",
  key: address,
  name: command ? `${address} (${command}, pid ${String(pid)})` : address,
  refs: [],
  paths: [],
  pid: pid || undefined,
});

/** One item a key (lsof lists a socket once a descriptor). */
const dedupe = (items: Item[]): Item[] => [...new Map(items.map((i) => [i.key, i])).values()];

/** The hosts file's entries, comments and blank lines left out, its spacing normalized. */
const hostsEntries: Parser = (output) =>
  lines(output)
    .map((l) => l.replace(/#.*/, "").trim().split(/\s+/).join(" "))
    .filter((l) => l !== "")
    .map((l) => ({ kind: "hosts-entry", key: l, name: l, refs: l.split(" ").slice(1), paths: [] }));

/** The routing table's network routes (the host and cloned ones, the kernel's own churn, left
 *  out): `ip -o route` lines, `netstat -rn` rows, or a Windows probe's JSON. */
const routes: Parser = (output) => {
  const json = jsonLines(output);
  if (json.length > 0) {
    return json
      .filter((r) => !/\/32$|\/128$/.test(str(r.DestinationPrefix)))
      .map((r) => route(`${str(r.DestinationPrefix)} via ${str(r.NextHop)} ${str(r.InterfaceAlias)}`));
  }
  if (/^Destination\s+Gateway/m.test(output)) {
    return lines(output).flatMap((l) => {
      const [dest, gateway, flags, iface] = l.trim().split(/\s+/);
      if (!iface || dest === "Destination" || !/U/.test(flags) || /[HW]/.test(flags)) return [];
      return [route(`${dest} ${gateway.startsWith("link#") ? "link" : gateway} ${iface}`)];
    });
  }
  return lines(output).map((l) => route(l.replace(/\s+(proto|metric|src|scope|linkdown)\s+\S*/g, "").trim()));
};

const route = (r: string): Item => ({ kind: "route", key: r, name: r, refs: [], paths: [] });

/** The login keychain's items of the launcher's services (`<service>\t<account>`), or Secret
 *  Service's (`label = `, `attribute.service = `, `attribute.username = ` lines), or Windows'
 *  Credential Manager's (`Target: ` lines). */
const credentials: Parser = (output) => {
  const out: Item[] = [];
  let service = "";
  for (const l of lines(output)) {
    const tab = /^([^\t]+)\t(.*)$/.exec(l);
    const attr = /^attribute\.(service|username) = (.*)$/.exec(l);
    const target = /^\s*Target:\s*(.*)$/.exec(l);
    const name = tab ? `${tab[1]} ${tab[2]}` : target ? target[1] : "";
    if (attr?.[1] === "service") service = attr[2];
    const item = name || (attr?.[1] === "username" ? `${service} ${attr[2]}` : "");
    if (item) out.push({ kind: "keychain-item", key: item, name: item, refs: item.split(" "), paths: [] });
  }
  return dedupe(out);
};

/** iptables-save's chains and rules (its counters and comments left out). */
const iptablesRules: Parser = (output) =>
  lines(output)
    .filter((l) => /^(\*|:|-A )/.test(l))
    .reduce<{ table: string; items: Item[] }>(
      (acc, l) => {
        if (l.startsWith("*")) return { ...acc, table: l.slice(1) };
        const rule = `${acc.table} ${l.replace(/ \[\d+:\d+\]$/, "")}`;
        acc.items.push({ kind: "iptables-rule", key: rule, name: rule, refs: [], paths: [] });
        return acc;
      },
      { table: "", items: [] },
    ).items;

export const HOST_PARSERS: Record<string, Parser> = {
  "docker-container": dockerContainer,
  "docker-network": dockerNetwork,
  "docker-volume": dockerVolume,
  "docker-image": dockerImage,
  "vagrant-machine": vagrantMachine,
  "vagrant-box": vagrantBox,
  "vagrant-dotfolder": (o) => lines(o).map((l) => pathItem("vagrant-dotfolder", l.trim())),
  "run-file": (o) => lines(o).map((l) => pathItem("run-file", l.trim())),
  "temp-folder": (o) => lines(o).map((l) => pathItem("temp-folder", l.trim())),
  process: processes,
  "listening-port": listeningPorts,
  "hosts-entry": hostsEntries,
  route: routes,
  "pf-anchor": lineItems("pf-anchor"),
  // Paths (a desktop entry, a launch agent), or a Windows probe's registry keys ({Name}).
  "os-registration": (o) => {
    const json = jsonLines(o);
    return json.length > 0 ? json.map((r) => pathItem("os-registration", str(r.Name))) : lines(o).map((l) => pathItem("os-registration", l.trim()));
  },
  "nft-table": lineItems("nft-table", (l) => l.startsWith("table ")),
  "iptables-rule": iptablesRules,
  "keychain-item": credentials,
};

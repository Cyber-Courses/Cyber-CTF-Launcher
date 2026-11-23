// The supervisor `runBounded` (host.mjs) runs a program under: `node bounded.mjs <ms> <program>
// [args…]` runs it without a shell, stdin the null device, reads its output, and prints one line
// of JSON, `{ out, code, timedOut, missing }`. At <ms> it ends the program with everything the
// program started (taskkill /T on Windows; elsewhere each process under it, by `ps`) and answers
// `timedOut`: a Docker CLI plugin (Compose) outlives a docker.exe that is ended alone. The program
// stays in the caller's process group, so the runner's end of a spec's group (run.mjs) takes it
// too. No dependency of its own.
import { execFileSync, spawn } from "node:child_process";

const [ms, program, ...args] = process.argv.slice(2);
const windows = process.platform === "win32";

/** `pid` and every process under it, by ps's parent links; `pid` alone when ps can't be read. */
function tree(pid) {
  let table = "";
  try {
    table = execFileSync("ps", ["-A", "-o", "pid=,ppid="], { encoding: "utf8", stdio: ["ignore", "pipe", "ignore"], timeout: 5000 });
  } catch {
    /* no process table: the program alone */
  }
  const children = new Map();
  for (const line of table.split("\n")) {
    const [child, parent] = line.trim().split(/\s+/).map(Number);
    if (child && !Number.isNaN(parent)) children.set(parent, [...(children.get(parent) ?? []), child]);
  }
  const found = [pid];
  for (let i = 0; i < found.length; i++) found.push(...(children.get(found[i]) ?? []));
  return found;
}

/** Ends `pid` with everything it started. */
function endTree(pid) {
  if (windows) {
    try {
      execFileSync("taskkill", ["/T", "/F", "/PID", String(pid)], { stdio: "ignore", windowsHide: true, timeout: 10000 });
    } catch {
      /* gone already */
    }
    return;
  }
  for (const p of tree(pid)) {
    try {
      process.kill(p, "SIGKILL");
    } catch {
      /* gone already */
    }
  }
}

let answered = false;
/** Prints the answer, once, and ends. */
function answer(r) {
  if (answered) return;
  answered = true;
  process.stdout.write(`${JSON.stringify(r)}\n`, () => process.exit(0));
}

let out = "";
let exitCode; // undefined until the program exits
let closed = false;
let late = null;
const finish = () => answer({ out, code: exitCode ?? null, timedOut: false, missing: false });

const child = spawn(program, args, { stdio: ["ignore", "pipe", "ignore"], windowsHide: true });
child.stdout.setEncoding("utf8");
child.stdout.on("data", (d) => (out += d));
child.on("error", (e) => answer({ out, code: null, timedOut: false, missing: e.code === "ENOENT" }));
const timer = setTimeout(() => {
  endTree(child.pid);
  answer({ out, code: null, timedOut: true, missing: false });
}, Number(ms));
child.stdout.on("close", () => {
  closed = true;
  if (exitCode !== undefined) {
    clearTimeout(late);
    finish();
  }
});
child.on("exit", (code) => {
  clearTimeout(timer);
  exitCode = code;
  if (closed) finish();
  // Something it started holds its output open: what came is the answer, a moment on.
  else late = setTimeout(finish, 1000);
});

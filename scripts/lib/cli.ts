// What the TypeScript runners under scripts/ share with the shell ones (lib/suite.sh's
// suite_help): `-h` / `--help` prints the runner's header, the comment its file starts with.
import { readFileSync } from "node:fs";

/** The comment a script starts with, each line without its `//`: what it does, how it's run. */
export function header(source: string): string {
  const lines: string[] = [];
  for (const line of source.split("\n")) {
    if (!line.startsWith("//")) break;
    lines.push(line.replace(/^\/\/ ?/, ""));
  }
  return lines.join("\n");
}

/** Whether `args` ask for help; when they do, the header of the script at `url` (its
 *  `import.meta.url`) is printed. */
export function helpAsked(url: string, args: readonly string[]): boolean {
  if (!args.includes("-h") && !args.includes("--help")) return false;
  console.log(header(readFileSync(new URL(url), "utf8")));
  return true;
}

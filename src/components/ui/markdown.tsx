"use client";

import type { JSX, ReactNode } from "react";
import { openExternal } from "@/lib/failure";

/**
 * Minimal, dependency-free Markdown renderer that emits React elements (never
 * dangerouslySetInnerHTML), so lab write-ups render safely. Covers the common subset:
 * headings, fenced code, lists, blockquotes, rules, paragraphs; inline code/bold/italic/links.
 */

// A link's URL may hold one level of balanced parentheses (`Foo_(bar)`), as in CommonMark.
const LINK = /\[([^\]]+)\]\(((?:[^()\s]|\([^()\s]*\))+)\)/;
const INLINE = new RegExp(`(\`[^\`]+\`)|(\\*\\*[^*]+\\*\\*)|(\\*[^*]+\\*)|(${LINK.source})`, "g");
/** A heading: one to six hashes, then a space. Deeper levels render as the third. */
const HEADING = /^(#{1,6})\s+(.*)$/;

/** Links that open in the browser: the web and mail only. A write-up is third-party content,
 *  so a `javascript:` or `data:` URL stays text and never reaches the system opener. */
const SAFE_URL = /^(https?:\/\/|mailto:)/i;

function inline(text: string, k: string): ReactNode[] {
  const out: ReactNode[] = [];
  let last = 0;
  let i = 0;
  let m: RegExpExecArray | null;
  INLINE.lastIndex = 0;
  while ((m = INLINE.exec(text))) {
    if (m.index > last) out.push(text.slice(last, m.index));
    out.push(span(m[0], `${k}-${String(i)}`));
    last = m.index + m[0].length;
    i++;
  }
  if (last < text.length) out.push(text.slice(last));
  return out;
}

/** One inline match: code, bold, italic, or a link (an unsafe link stays its text). */
function span(t: string, key: string): ReactNode {
  if (t.startsWith("`")) {
    return (
      <code key={key} className="rounded-xs bg-glass-2 px-1 py-0.5 font-mono text-[0.85em] text-foreground shadow-[inset_0_0_0_1px_var(--border)]">
        {t.slice(1, -1)}
      </code>
    );
  }
  if (t.startsWith("**")) {
    return (
      <strong key={key} className="font-semibold text-foreground">
        {t.slice(2, -2)}
      </strong>
    );
  }
  if (t.startsWith("*")) return <em key={key}>{t.slice(1, -1)}</em>;
  const mm = LINK.exec(t);
  // INLINE only matches the forms above and links: a match is always one.
  if (!mm) return t;
  const href = mm[2];
  if (!SAFE_URL.test(href)) return mm[1];
  return (
    <a
      key={key}
      href={href}
      onClick={(e) => {
        e.preventDefault();
        openExternal(href);
      }}
      className="cursor-pointer text-link underline underline-offset-4 decoration-[color-mix(in_oklab,var(--link)_40%,transparent)] hover:decoration-current"
    >
      {mm[1]}
    </a>
  );
}

const BULLET = /^\s*[-*]\s+/;
const NUMBERED = /^\s*\d+\.\s+/;

/** A block read from `lines` at `i`: its element and the index of the line after it. */
type Block = { node: ReactNode; next: number };
/** Reads the block starting at `i`, or null when the line doesn't start this kind of block. */
type BlockReader = (lines: string[], i: number, key: string) => Block | null;

/** The lines from `i` while `test` holds, each mapped by `map`, and the index after them. */
function takeLines(lines: string[], i: number, test: (line: string) => boolean, map: (line: string) => string = (l) => l) {
  const buf: string[] = [];
  while (i < lines.length && test(lines[i])) buf.push(map(lines[i++]));
  return { buf, next: i };
}

const fence: BlockReader = (lines, i, key) => {
  if (!lines[i].startsWith("```")) return null;
  const { buf, next } = takeLines(lines, i + 1, (l) => !l.startsWith("```"));
  return {
    node: (
      <pre key={key} className="surface-log overflow-auto rounded-control px-3.5 py-3 font-mono text-[0.75rem] leading-[1.8] text-muted-foreground">
        {buf.join("\n")}
      </pre>
    ),
    next: next + 1,
  };
};

const HEADING_CLASS = [
  "serif-title mt-6 text-[1.5rem] text-foreground first:mt-0",
  "serif-title mt-5 text-[1.25rem] text-foreground",
  "mt-4 text-sm font-semibold",
];

const heading: BlockReader = (lines, i, key) => {
  const h = HEADING.exec(lines[i]);
  if (!h) return null;
  const lvl = Math.min(h[1].length, 3);
  const Tag = `h${String(lvl + 1)}` as keyof JSX.IntrinsicElements;
  return {
    node: (
      <Tag key={key} className={HEADING_CLASS[lvl - 1]}>
        {inline(h[2], key)}
      </Tag>
    ),
    next: i + 1,
  };
};

const rule: BlockReader = (lines, i, key) =>
  /^(-{3,}|\*{3,})$/.test(lines[i].trim()) ? { node: <hr key={key} className="my-4 border-border" />, next: i + 1 } : null;

const quote: BlockReader = (lines, i, key) => {
  if (!lines[i].startsWith(">")) return null;
  const { buf, next } = takeLines(
    lines,
    i,
    (l) => l.startsWith(">"),
    (l) => l.replace(/^>\s?/, ""),
  );
  return {
    node: (
      <blockquote key={key} className="border-l-2 border-jewel/40 pl-3 text-muted-foreground">
        {inline(buf.join(" "), key)}
      </blockquote>
    ),
    next,
  };
};

/** A list block: the lines starting with `marker`, as a bullet (`ul`) or numbered (`ol`) list. */
function list(marker: RegExp, kind: "ul" | "ol"): BlockReader {
  return (lines, i, key) => {
    if (!marker.test(lines[i])) return null;
    const { buf, next } = takeLines(
      lines,
      i,
      (l) => marker.test(l),
      (l) => l.replace(marker, ""),
    );
    const items = buf.map((it, j) => <li key={j}>{inline(it, `${key}-${String(j)}`)}</li>);
    const node =
      kind === "ol" ? (
        <ol key={key} className="list-decimal space-y-1 pl-5">
          {items}
        </ol>
      ) : (
        <ul key={key} className="list-disc space-y-1 pl-5">
          {items}
        </ul>
      );
    return { node, next };
  };
}

/** The lines up to the next blank line or other block. A heading ends the paragraph; any other
 *  line starting with "#" (`#hashtag`) is its text. Refusing them all left such a line in no
 *  block, and the parser spun forever. */
const paragraph = (lines: string[], i: number, key: string): Block => {
  const { buf, next } = takeLines(
    lines,
    i,
    (l) => l.trim() !== "" && !HEADING.test(l) && !l.startsWith("```") && !l.startsWith(">") && !BULLET.test(l) && !NUMBERED.test(l),
  );
  return {
    node: (
      <p key={key} className="leading-relaxed">
        {inline(buf.join(" "), key)}
      </p>
    ),
    next,
  };
};

/** The block readers, tried in order; a line none of them takes starts a paragraph. */
const READERS: BlockReader[] = [fence, heading, rule, quote, list(BULLET, "ul"), list(NUMBERED, "ol")];

export function Markdown({ content, className }: { content: string; className?: string }) {
  const lines = content.replace(/\r\n/g, "\n").split("\n");
  const blocks: ReactNode[] = [];
  let i = 0;
  while (i < lines.length) {
    if (lines[i].trim() === "") {
      i++;
      continue;
    }
    const key = `b${String(blocks.length)}`;
    const block = READERS.reduce<Block | null>((found, read) => found ?? read(lines, i, key), null) ?? paragraph(lines, i, key);
    blocks.push(block.node);
    i = block.next;
  }
  return <div className={className}>{blocks}</div>;
}

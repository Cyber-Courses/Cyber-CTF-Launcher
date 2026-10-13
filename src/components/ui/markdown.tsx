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
    const t = m[0];
    if (t.startsWith("`")) {
      out.push(
        <code key={`${k}-${i}`} className="rounded-xs bg-glass-2 px-1 py-0.5 font-mono text-[0.85em] text-foreground shadow-[inset_0_0_0_1px_var(--border)]">
          {t.slice(1, -1)}
        </code>,
      );
    } else if (t.startsWith("**")) {
      out.push(
        <strong key={`${k}-${i}`} className="font-semibold text-foreground">
          {t.slice(2, -2)}
        </strong>,
      );
    } else if (t.startsWith("*")) {
      out.push(<em key={`${k}-${i}`}>{t.slice(1, -1)}</em>);
    } else {
      const mm = LINK.exec(t)!;
      const href = mm[2];
      if (!SAFE_URL.test(href)) {
        out.push(mm[1]);
        last = m.index + t.length;
        i++;
        continue;
      }
      out.push(
        <a
          key={`${k}-${i}`}
          href={href}
          onClick={(e) => {
            e.preventDefault();
            openExternal(href);
          }}
          className="cursor-pointer text-link underline underline-offset-4 decoration-[color-mix(in_oklab,var(--link)_40%,transparent)] hover:decoration-current"
        >
          {mm[1]}
        </a>,
      );
    }
    last = m.index + t.length;
    i++;
  }
  if (last < text.length) out.push(text.slice(last));
  return out;
}

export function Markdown({ content, className }: { content: string; className?: string }) {
  const lines = content.replace(/\r\n/g, "\n").split("\n");
  const blocks: ReactNode[] = [];
  let i = 0;
  const key = () => `b${blocks.length}`;

  while (i < lines.length) {
    const line = lines[i];
    if (line.trim() === "") {
      i++;
      continue;
    }
    if (line.startsWith("```")) {
      const buf: string[] = [];
      i++;
      while (i < lines.length && !lines[i].startsWith("```")) buf.push(lines[i++]);
      i++;
      blocks.push(
        <pre key={key()} className="surface-log overflow-auto rounded-control px-3.5 py-3 font-mono text-[0.75rem] leading-[1.8] text-muted-foreground">
          {buf.join("\n")}
        </pre>,
      );
      continue;
    }
    const h = HEADING.exec(line);
    if (h) {
      const lvl = Math.min(h[1].length, 3);
      const cls =
        lvl === 1
          ? "serif-title mt-6 text-[1.5rem] text-foreground first:mt-0"
          : lvl === 2
            ? "serif-title mt-5 text-[1.25rem] text-foreground"
            : "mt-4 text-sm font-semibold";
      const Tag = `h${lvl + 1}` as keyof JSX.IntrinsicElements;
      blocks.push(
        <Tag key={key()} className={cls}>
          {inline(h[2], key())}
        </Tag>,
      );
      i++;
      continue;
    }
    if (/^(-{3,}|\*{3,})$/.test(line.trim())) {
      blocks.push(<hr key={key()} className="my-4 border-border" />);
      i++;
      continue;
    }
    if (line.startsWith(">")) {
      const buf: string[] = [];
      while (i < lines.length && lines[i].startsWith(">")) buf.push(lines[i++].replace(/^>\s?/, ""));
      blocks.push(
        <blockquote key={key()} className="border-l-2 border-jewel/40 pl-3 text-muted-foreground">
          {inline(buf.join(" "), key())}
        </blockquote>,
      );
      continue;
    }
    if (/^\s*[-*]\s+/.test(line)) {
      const items: string[] = [];
      while (i < lines.length && /^\s*[-*]\s+/.test(lines[i])) items.push(lines[i++].replace(/^\s*[-*]\s+/, ""));
      blocks.push(
        <ul key={key()} className="list-disc space-y-1 pl-5">
          {items.map((it, j) => (
            <li key={j}>{inline(it, `${key()}-${j}`)}</li>
          ))}
        </ul>,
      );
      continue;
    }
    if (/^\s*\d+\.\s+/.test(line)) {
      const items: string[] = [];
      while (i < lines.length && /^\s*\d+\.\s+/.test(lines[i])) items.push(lines[i++].replace(/^\s*\d+\.\s+/, ""));
      blocks.push(
        <ol key={key()} className="list-decimal space-y-1 pl-5">
          {items.map((it, j) => (
            <li key={j}>{inline(it, `${key()}-${j}`)}</li>
          ))}
        </ol>,
      );
      continue;
    }
    const buf: string[] = [];
    while (
      i < lines.length &&
      lines[i].trim() !== "" &&
      // A heading ends the paragraph; any other line starting with "#" (`#hashtag`) is its
      // text. Refusing them all left such a line in no block, and the parser spun forever.
      !HEADING.test(lines[i]) &&
      !lines[i].startsWith("```") &&
      !lines[i].startsWith(">") &&
      !/^\s*[-*]\s+/.test(lines[i]) &&
      !/^\s*\d+\.\s+/.test(lines[i])
    ) {
      buf.push(lines[i++]);
    }
    blocks.push(
      <p key={key()} className="leading-relaxed">
        {inline(buf.join(" "), key())}
      </p>,
    );
  }

  return <div className={className}>{blocks}</div>;
}

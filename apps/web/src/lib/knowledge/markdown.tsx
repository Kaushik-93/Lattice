import type { ReactNode } from "react";

/** One run of inline text. Raw HTML is never interpreted: everything renders as text. */
export type Inline =
  | { kind: "text"; text: string }
  | { kind: "link"; target: string; label: string }
  | { kind: "code"; text: string }
  | { kind: "strong"; text: string }
  | { kind: "em"; text: string };

export type Block = { kind: "paragraph"; lines: Inline[][] } | { kind: "list"; items: Inline[][] };

const INLINE =
  /\[\[([^\]\n|]+?)(?:\|([^\]\n]+?))?\]\]|`([^`\n]+)`|\*\*([^*\n]+)\*\*|\*([^*\n]+)\*/g;

export function parseInline(text: string): Inline[] {
  const out: Inline[] = [];
  let last = 0;
  for (const m of text.matchAll(INLINE)) {
    const at = m.index ?? 0;
    if (at > last) out.push({ kind: "text", text: text.slice(last, at) });
    if (m[1] !== undefined) {
      const target = m[1].trim();
      out.push({ kind: "link", target, label: (m[2] ?? m[1]).trim() });
    } else if (m[3] !== undefined) out.push({ kind: "code", text: m[3] });
    else if (m[4] !== undefined) out.push({ kind: "strong", text: m[4] });
    else if (m[5] !== undefined) out.push({ kind: "em", text: m[5] });
    last = at + m[0].length;
  }
  if (last < text.length) out.push({ kind: "text", text: text.slice(last) });
  return out;
}

/** Paragraphs (blank-line separated) and bullet lists ("- " or "* "). */
export function parseBlocks(body: string): Block[] {
  return body
    .split(/\n\s*\n/)
    .map((chunk) => chunk.split("\n").filter((l) => l.trim() !== ""))
    .filter((lines) => lines.length > 0)
    .map((lines): Block => {
      const bullets = lines.every((l) => /^\s*[-*]\s+/.test(l));
      return bullets
        ? { kind: "list", items: lines.map((l) => parseInline(l.replace(/^\s*[-*]\s+/, ""))) }
        : { kind: "paragraph", lines: lines.map(parseInline) };
    });
}

export interface LinkTarget {
  resolved: boolean;
  /** Not yet approved: shown with a dashed underline. */
  tentative?: boolean;
}

/** A note body: `[[links]]` become buttons that open the linked note (or offer to create it). */
export function NoteBody({
  body,
  target,
  onLink,
}: {
  body: string;
  target: (title: string) => LinkTarget;
  onLink: (title: string) => void;
}) {
  const inline = (runs: Inline[]): ReactNode[] =>
    runs.map((r, i) => {
      switch (r.kind) {
        case "text":
          return r.text;
        case "code":
          return (
            <code key={i} className="rounded bg-secondary px-1 py-0.5 font-mono text-[0.9em]">
              {r.text}
            </code>
          );
        case "strong":
          return <strong key={i}>{r.text}</strong>;
        case "em":
          return <em key={i}>{r.text}</em>;
        case "link": {
          const t = target(r.target);
          return (
            <button
              key={i}
              type="button"
              onClick={() => onLink(r.target)}
              title={
                t.resolved ? `Open ${r.target}` : `No note called “${r.target}” yet — create it`
              }
              className={
                t.resolved
                  ? `font-medium text-primary underline-offset-2 hover:underline ${t.tentative ? "underline decoration-warning decoration-dashed" : ""}`
                  : "text-subtle-foreground italic underline decoration-dashed underline-offset-2 hover:text-foreground"
              }
            >
              {r.label}
            </button>
          );
        }
      }
    });
  return (
    <div className="flex flex-col gap-2.5 text-[15px] leading-relaxed text-foreground">
      {parseBlocks(body).map((block, i) =>
        block.kind === "list" ? (
          <ul key={i} className="ml-5 list-disc">
            {block.items.map((item, j) => (
              <li key={j}>{inline(item)}</li>
            ))}
          </ul>
        ) : (
          <p key={i}>
            {block.lines.map((line, j) => (
              <span key={j}>
                {j > 0 && <br />}
                {inline(line)}
              </span>
            ))}
          </p>
        ),
      )}
    </div>
  );
}

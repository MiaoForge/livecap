import { useState } from "react";
import type { ReactNode } from "react";
import { time } from "./clientApi";

type NotesProps = {
  text: string;
  seek?: (ms: number) => void;
  sourceTimes?: readonly (number | null)[];
  plain?: boolean;
};

type Reference = { label: string; ms?: number; source: string };

function CitationGroup({
  references,
  seek,
}: {
  references: Reference[];
  seek?: NotesProps["seek"];
}) {
  const [expanded, setExpanded] = useState(false);
  const [first, ...rest] = references;
  const link = (reference: Reference) =>
    reference.ms != null && seek ? (
      <button
        type="button"
        className="note-citation"
        aria-label={`回听 ${reference.label}（来源 ${reference.source}）`}
        onClick={() => seek(reference.ms!)}
      >
        {reference.label}
      </button>
    ) : (
      reference.label
    );
  return (
    <span className="note-citation-group">
      {link(first)}
      {!!rest.length && (
        <button
          type="button"
          className="note-citation-more"
          aria-expanded={expanded}
          aria-label={
            expanded ? "收起其他来源" : `查看另外 ${rest.length} 个来源`
          }
          onClick={() => setExpanded(!expanded)}
        >
          {expanded ? "收起" : `+${rest.length}`}
        </button>
      )}
      {expanded &&
        rest.map((reference, i) => <span key={i}>{link(reference)}</span>)}
    </span>
  );
}

export function NotesDocument({
  text,
  seek,
  sourceTimes,
  plain = false,
}: NotesProps) {
  function timestamp(label: string, ms: number, key: number, source?: string) {
    return seek ? (
      <button
        key={key}
        type="button"
        className="note-citation"
        aria-label={
          source ? `回听 ${label}（来源 ${source}）` : `回听 ${label}`
        }
        onClick={() => seek(ms)}
      >
        {label}
      </button>
    ) : (
      label
    );
  }
  function inline(value: string): ReactNode[] {
    const tokens = plain
      ? /((?:\[S\d+\](?:[ \t]+\[?\b\d{1,3}:\d{2}\b\]?)?(?:[ \t]*(?=\[S\d+\]))?)+|\[?\b\d{1,3}:\d{2}\b\]?)/g
      : /(\*\*[^*]+\*\*|(?:\[S\d+\](?:[ \t]+\[?\b\d{1,3}:\d{2}\b\]?)?(?:[ \t]*(?=\[S\d+\]))?)+|\[?\b\d{1,3}:\d{2}\b\]?)/g;
    return value.split(tokens).map((part, i) => {
      if (!plain && part.startsWith("**") && part.endsWith("**"))
        return <strong key={i}>{inline(part.slice(2, -2))}</strong>;
      if (/^\[S\d+\]/.test(part)) {
        const references = Array.from(part.matchAll(/\[S(\d+)\]/g), (match) => {
          const ms = sourceTimes?.[Number(match[1]) - 1] ?? undefined;
          return {
            label: ms == null ? match[0] : time(ms),
            ms,
            source: "S" + match[1],
          };
        });
        return references.some((reference) => reference.ms != null) ? (
          <CitationGroup key={i} references={references} seek={seek} />
        ) : (
          part
        );
      }
      const clock = part.match(/^\[?(\d{1,3}):(\d{2})\]?$/);
      return clock
        ? timestamp(part, (Number(clock[1]) * 60 + Number(clock[2])) * 1000, i)
        : part;
    });
  }
  if (plain) return <pre className="record-plain-note">{inline(text)}</pre>;
  const lines = text.split("\n"),
    blocks: ReactNode[] = [];
  let i = 0;
  while (i < lines.length) {
    const line = lines[i].trim();
    if (!line) {
      i++;
      continue;
    }
    if (line.startsWith("|") && lines[i + 1]?.match(/^\s*\|?[\s:|-]+\|\s*$/)) {
      const cells = (s: string) =>
        s
          .trim()
          .replace(/^\||\|$/g, "")
          .split("|")
          .map((x) => x.trim());
      const head = cells(line);
      i += 2;
      const rows: string[][] = [];
      while (i < lines.length && lines[i].trim().startsWith("|"))
        rows.push(cells(lines[i++]));
      blocks.push(
        <table key={i}>
          <thead>
            <tr>
              {head.map((x, k) => (
                <th key={k}>{inline(x)}</th>
              ))}
            </tr>
          </thead>
          <tbody>
            {rows.map((row, k) => (
              <tr key={k}>
                {row.map((x, j) => (
                  <td key={j}>{inline(x)}</td>
                ))}
              </tr>
            ))}
          </tbody>
        </table>,
      );
      continue;
    }
    if (/^#{1,6}\s/.test(line))
      blocks.push(<h3 key={i}>{inline(line.replace(/^#+\s/, ""))}</h3>);
    else if (/^[-*]\s/.test(line))
      blocks.push(
        <div className="note-bullet" key={i}>
          <span>•</span>
          <p>{inline(line.slice(2))}</p>
        </div>,
      );
    else blocks.push(<p key={i}>{inline(line)}</p>);
    i++;
  }
  const sections: ReactNode[] = [];
  let content: ReactNode[] = [];
  let title: ReactNode = null;
  function finishSection() {
    if (!title && !content.length) return;
    sections.push(
      <section
        key={sections.length}
        className={
          "note-section " +
          (sections.length === 0 && title ? "note-summary-section" : "")
        }
      >
        {title}
        {content}
      </section>,
    );
    content = [];
    title = null;
  }
  for (const block of blocks) {
    if (
      typeof block === "object" &&
      block &&
      "type" in block &&
      block.type === "h3"
    ) {
      finishSection();
      title = block;
    } else content.push(block);
  }
  finishSection();
  return <div className="native-note">{sections}</div>;
}

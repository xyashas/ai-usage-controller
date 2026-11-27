import { useState, type ReactNode } from "react";
import { api } from "../api";

/** Inline: **bold**, *italic*, `code`. Rendered as elements, never as HTML. */
function inline(text: string): ReactNode[] {
  const out: ReactNode[] = [];
  const re = /(\*\*[^*]+\*\*|`[^`]+`|\*[^*\s][^*]*\*)/g;
  let last = 0;
  let m: RegExpExecArray | null;
  let k = 0;
  while ((m = re.exec(text))) {
    if (m.index > last) out.push(text.slice(last, m.index));
    const t = m[0];
    if (t.startsWith("**")) out.push(<strong key={k++} className="font-semibold text-fg">{t.slice(2, -2)}</strong>);
    else if (t.startsWith("`")) out.push(<code key={k++} className="rounded-md bg-white/[0.08] px-1 py-px font-mono text-[0.92em]">{t.slice(1, -1)}</code>);
    else out.push(<em key={k++}>{t.slice(1, -1)}</em>);
    last = m.index + t.length;
  }
  if (last < text.length) out.push(text.slice(last));
  return out;
}

function CodeBlock({ code, lang }: { code: string; lang: string }) {
  const [copied, setCopied] = useState(false);
  return (
    <div className="group relative">
      <pre className="select-text overflow-x-auto rounded-xl bg-black/50 px-3 py-2.5 font-mono text-[11.5px] leading-relaxed ring-1 ring-white/[0.06]">
        {lang && <span className="absolute right-2 top-1.5 text-[10px] text-mute group-hover:hidden">{lang}</span>}
        <code>{code}</code>
      </pre>
      <button
        className="absolute right-1.5 top-1.5 hidden rounded-lg bg-white/10 px-2 py-0.5 text-[10.5px] text-fg group-hover:block"
        onClick={() => api.askCopy(code).then(() => { setCopied(true); setTimeout(() => setCopied(false), 1200); })}
      >
        {copied ? "Copied" : "Copy"}
      </button>
    </div>
  );
}

/** Enough Markdown for chat answers: paragraphs, headings, lists, code blocks. */
export function Markdown({ text }: { text: string }) {
  const blocks: ReactNode[] = [];
  const lines = text.replace(/\r/g, "").split("\n");
  let i = 0;
  let k = 0;
  while (i < lines.length) {
    const line = lines[i];
    const fence = line.match(/^\s*```(\w*)/);
    if (fence) {
      const body: string[] = [];
      i++;
      while (i < lines.length && !/^\s*```/.test(lines[i])) body.push(lines[i++]);
      i++;
      blocks.push(<CodeBlock key={k++} code={body.join("\n")} lang={fence[1]} />);
      continue;
    }
    if (/^\s*([-*•]|\d+[.)])\s+/.test(line)) {
      const ordered = /^\s*\d/.test(line);
      const items: string[] = [];
      while (i < lines.length && /^\s*([-*•]|\d+[.)])\s+/.test(lines[i])) items.push(lines[i++].replace(/^\s*([-*•]|\d+[.)])\s+/, ""));
      const List = ordered ? "ol" : "ul";
      blocks.push(
        <List key={k++} className={`space-y-1 pl-5 ${ordered ? "list-decimal" : "list-disc"} marker:text-mute`}>
          {items.map((it, j) => <li key={j}>{inline(it)}</li>)}
        </List>,
      );
      continue;
    }
    const h = line.match(/^#{1,4}\s+(.*)/) ?? line.match(/^\s*\*\*([^*]+)\*\*:?\s*$/);
    if (h) { blocks.push(<p key={k++} className="pt-1 text-[13px] font-semibold text-fg">{inline(h[1])}</p>); i++; continue; }
    if (!line.trim()) { i++; continue; }
    const para: string[] = [];
    while (i < lines.length && lines[i].trim() && !/^\s*(```|[-*•]\s|\d+[.)]\s|#)/.test(lines[i]) && !(para.length && /^\s*\*\*[^*]+\*\*:?\s*$/.test(lines[i]))) para.push(lines[i++]);
    blocks.push(<p key={k++}>{inline(para.join(" "))}</p>);
  }
  return <div className="space-y-2 text-[12.5px] leading-relaxed text-fg/90">{blocks}</div>;
}

/** Plain text for reading aloud. */
export function plain(md: string) {
  return md.replace(/```[\s\S]*?```/g, " (code) ").replace(/[*`#>]/g, "").replace(/\n+/g, ". ");
}

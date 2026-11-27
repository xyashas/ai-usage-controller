import { useEffect, useState } from "react";
import { api } from "../api";
import type { Dest, DestOption, HandoffPreviewData, Tool } from "../types";
import { tint, toolName } from "../theme";
import { ToolMark } from "./ToolMark";
import { BackIcon, Check } from "./icons";
import { preferredDest, rememberDest } from "../prefs";

export { toolName };
export type Draft = { source?: Tool; target: Tool; preview: Promise<HandoffPreviewData> };

const action: Record<Dest, (t: Tool, label: string) => string> = {
  app: (_t, label) => `Open ${label}`,
  terminal: () => "Open in terminal",
  web: (t) => `Open ${toolName[t]} on the web`,
};

const hint: Record<Dest, string> = {
  app: "Copies the handoff and opens the app. Paste it into a new session there.",
  terminal: "Saves .aiuc-handoff.md in the project (hidden from git) and starts the CLI there, already reading it.",
  web: "Copies the handoff and opens the web app. Cloud sessions see what's pushed to GitHub, so push your branch first.",
};

export function HandoffPreview({ draft, onBack }: { draft: Draft; onBack: () => void }) {
  const [data, setData] = useState<HandoffPreviewData>();
  const [prompt, setPrompt] = useState("");
  const [dest, setDest] = useState<DestOption>();
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);
  const [done, setDone] = useState<string | null>(null);
  useEffect(() => {
    let current = true;
    draft.preview.then((p) => {
      if (!current) return;
      setData(p);
      setPrompt(p.prompt);
      setDest(preferredDest(draft.target, p.destinations));
    }, (e) => { if (current) setError(String(e)); });
    return () => { current = false; };
  }, [draft]);

  const words = prompt.trim() ? prompt.trim().split(/\s+/u).length : 0;
  const tooLong = words >= 1500 || new TextEncoder().encode(prompt).length > 100000;
  const c = tint[draft.target];

  async function launch() {
    if (!data || !dest || busy) return;
    setBusy(true);
    setError("");
    try { setDone(await api.launchHandoff(data.id, prompt, dest.dest)); rememberDest(draft.target, dest.dest); }
    catch (e) { setError(String(e)); }
    finally { setBusy(false); }
  }

  if (done) {
    return (
      <section className="flex flex-col items-center gap-3 px-6 py-8 text-center fade-in">
        <span className="scale-[1.6]"><Check ok /></span>
        <h2 className="mt-2 text-[17px] font-semibold tracking-tight">Handed off to {toolName[draft.target]}</h2>
        <p className="max-w-[340px] text-[12.5px] leading-relaxed text-muted">{done}</p>
        <button className="btn btn-primary mt-2 min-w-[120px]" onClick={onBack}>Done</button>
      </section>
    );
  }

  return (
    <section className="flex flex-col gap-3 p-3">
      <header className="flex items-center gap-2 px-1">
        <button disabled={busy} className="icon-btn" aria-label="Back" onClick={onBack}><BackIcon /></button>
        <div className="min-w-0">
          <h2 className="flex items-center gap-1.5 text-[15px] font-semibold tracking-tight"><ToolMark tool={draft.target} />Continue in {toolName[draft.target]}</h2>
          <p className="truncate text-[11.5px] text-muted">{draft.source ? `From ${toolName[draft.source]}` : "From the repo's handoff"}{data?.place ? ` · ${data.place}` : ""}</p>
        </div>
      </header>

      {data ? (
        <>
          <div className="card !p-0" style={{ background: c.bg, color: c.ink }}>
            <textarea
              aria-label="Handoff prompt"
              className="block h-[230px] w-full resize-none bg-transparent px-4 py-3.5 font-mono text-[11.5px] leading-relaxed outline-none"
              value={prompt}
              spellCheck={false}
              autoCorrect="off"
              autoCapitalize="off"
              disabled={busy}
              onFocus={() => api.focus()}
              onChange={(e) => setPrompt(e.target.value)}
            />
            <p className={`tabular px-4 pb-2.5 text-right text-[10.5px] ${tooLong ? "font-semibold text-[#a3261b]" : "opacity-50"}`}>{words.toLocaleString()} / 1,499 words</p>
          </div>

          {data.destinations.length > 1 && (
            <div className="flex items-center justify-between gap-2 px-1">
              <span className="text-[12px] text-muted">Open in</span>
              <div className="seg" role="group" aria-label="Open in">
                {data.destinations.map((d) => (
                  <button key={d.dest} aria-pressed={dest?.dest === d.dest} onClick={() => setDest(d)}>{d.label}</button>
                ))}
              </div>
            </div>
          )}
          {dest && <p className="px-1 text-[11.5px] leading-relaxed text-muted">{hint[dest.dest]}</p>}

          <button disabled={busy || !words || tooLong || !dest} className="btn btn-primary w-full !py-2.5 text-[13px]" onClick={launch}>
            {busy ? "Opening…" : dest ? action[dest.dest](draft.target, dest.label) : "Continue"}
          </button>
        </>
      ) : !error && <p role="status" className="px-1 py-6 text-center text-[12px] text-muted">Reading the session…</p>}
      {error && <p role="alert" className="px-1 text-[12px] text-bad">{error}</p>}
    </section>
  );
}

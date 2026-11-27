import { useEffect, useState } from "react";
import { api } from "../api";
import type { Tool } from "../types";

export const toolName: Record<Tool, string> = { claude_code: "Claude Code", codex: "Codex", gemini: "Gemini" };
export type Draft = { source: Tool; target: Tool; preview: Promise<{ id: number; prompt: string }> };

export function HandoffPreview({ draft, onBack }: { draft: Draft; onBack: () => void }) {
  const [id, setId] = useState<number>();
  const [prompt, setPrompt] = useState("");
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);
  useEffect(() => {
    let current = true;
    draft.preview.then(p => {
      if (current) { setPrompt(p.prompt); setId(p.id); }
    }, e => { if (current) setError(String(e)); });
    return () => { current = false; };
  }, [draft]);
  const words = prompt.trim() ? prompt.trim().split(/\s+/u).length : 0;
  async function launch() {
    if (id == null || busy) return;
    setBusy(true);
    setError("");
    try { await api.launchHandoff(id, prompt); onBack(); }
    catch (e) { setError(String(e)); setBusy(false); }
  }
  return <section className="flex flex-col gap-4 p-4">
    <button disabled={busy} className="self-start text-xs text-muted" onClick={onBack}>Back</button>
    <h2 className="text-[18px] font-semibold">Preview handoff</h2>
    <p className="text-xs text-muted">{toolName[draft.source]} to {toolName[draft.target]}. Review what the next session will read.</p>
    {id != null ? <>
      <label className="flex flex-col gap-2 text-xs">Handoff prompt
        <textarea className="h-56 resize-none rounded-lg bg-fg/5 p-2 text-[13px]" value={prompt} spellCheck={false}
          autoCorrect="off" autoCapitalize="off" disabled={busy} onFocus={() => api.focus()} onChange={e => setPrompt(e.target.value)} />
      </label>
      <p className="text-xs text-muted">{words.toLocaleString()} / 1,499 words. Continuing saves .aiuc-handoff.md in the project and copies this text to your clipboard.</p>
      <button disabled={busy || !words || words >= 1500 || new TextEncoder().encode(prompt).length > 100000}
        className="rounded-full bg-fg py-2 text-sm font-semibold text-canvas" onClick={launch}>
        {busy ? "Opening terminal..." : `Continue in ${toolName[draft.target]}`}
      </button>
    </> : !error && <p role="status" className="text-xs">Reading the local session...</p>}
    {error && <p role="alert" className="text-xs text-bad">{error}</p>}
  </section>;
}

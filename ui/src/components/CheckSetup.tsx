import { ToolMark } from "./ToolMark";
import { useEffect, useState } from "react";
import type { Diagnostics, ToolDiagnostics } from "../types";
import { api } from "../api";
import { ago } from "../format";
import { BackIcon, Check } from "./icons";
import { ClaudeConnect } from "./ClaudeConnect";

// "How do I know it works?" — what the app can see, and what to compare it with.
const COMPARE: Record<string, { where: string; url?: string }> = {
  claude_code: { where: "claude.ai → Settings → Usage, or /usage in Claude Code", url: "https://claude.ai/settings/usage" },
  codex: { where: "/status inside Codex" },
  gemini: { where: "/stats inside Gemini CLI" },
};

function sourceName(s: string | null) {
  if (s === "claude_web") return "claude.ai";
  if (s === "claude_oauth") return "Claude Code login";
  if (s === "codex_log") return "Codex logs";
  return s ?? "";
}

function ToolCard({ t, d }: { t: ToolDiagnostics; d: Diagnostics }) {
  const logs = t.folders_found.length > 0;
  const gauge = t.gauge_at != null;
  const ok = t.tool === "claude_code" ? gauge && !t.live_error : logs || t.requests_total > 0;
  const cmp = COMPARE[t.tool];
  return (
    <div className="space-y-2 rounded-2xl bg-fg/[0.04] p-4 ring-1 ring-fg/[0.06]">
      <div className="flex items-center gap-2">
        <Check ok={ok ? true : logs || gauge ? false : null} />
        <span className="flex items-center gap-2 text-[13px] font-medium"><ToolMark tool={t.tool} />{t.label}</span>
        {gauge && t.gauge_used_pct != null && (
          <span className="tabular ml-auto text-[12px] text-muted">{Math.round(t.gauge_used_pct)}% used</span>
        )}
      </div>
      <ul className="space-y-1 pl-6 text-[11.5px] leading-relaxed text-muted">
        {t.tool === "claude_code" && (
          <li>
            Plan limits:{" "}
            {gauge ? (
              <span className="text-fg">from {sourceName(t.gauge_source)}, updated {ago(t.gauge_at)}</span>
            ) : d.claude_web ? (
              <span className="text-fg">connected, waiting for the first reading</span>
            ) : (
              <span className="text-warn">not connected</span>
            )}
          </li>
        )}
        {t.tool === "codex" && gauge && (
          <li>
            Plan limits: <span className="text-fg">from Codex logs, updated {ago(t.gauge_at)}</span>
          </li>
        )}
        <li>
          {t.tool === "claude_code" ? "Claude Code logs" : "Logs"}:{" "}
          {logs ? <span className="text-fg">found</span> : <span>not found{t.tool === "claude_code" ? " (fine if you only use claude.ai)" : ""}</span>}
        </li>
        {(logs || t.requests_total > 0) && (
          <li>
            Requests today: <span className="tabular text-fg">{t.requests_today}</span> · last {ago(t.last_request_at)}
          </li>
        )}
        {t.live_error && <li className="text-bad">{t.live_error}</li>}
        <li>
          Compare with: <span className="text-fg">{cmp.where}</span>
          {cmp.url && (
            <button className="ml-1 text-accent hover:underline" onClick={() => api.openUrl(cmp.url!)}>
              open
            </button>
          )}
        </li>
      </ul>
      {t.tool === "claude_code" && !gauge && !d.claude_web && <ClaudeConnect compact />}
    </div>
  );
}

export function CheckSetup({ onBack }: { onBack: () => void }) {
  const [d, setD] = useState<Diagnostics | null>(null);
  const [busy, setBusy] = useState(false);
  const [copied, setCopied] = useState(false);

  const load = async () => setD(await api.diagnostics());
  useEffect(() => {
    load();
    const id = setInterval(load, 3000);
    return () => clearInterval(id);
  }, []);

  async function refresh() {
    setBusy(true);
    try {
      await api.refreshNow();
      await load();
    } finally {
      setTimeout(() => setBusy(false), 600);
    }
  }

  function copy() {
    const text = JSON.stringify(d, null, 2);
    const ta = document.createElement("textarea");
    ta.value = text;
    document.body.appendChild(ta);
    ta.select();
    document.execCommand("copy");
    ta.remove();
    setCopied(true);
    setTimeout(() => setCopied(false), 1500);
  }

  return (
    <div className="flex flex-col gap-4 p-4">
      <div className="flex items-center justify-between">
        <button onClick={onBack} className="flex items-center gap-1 text-[12px] text-muted hover:text-fg">
          <BackIcon /> Back
        </button>
        <span className="text-[11px] text-muted">v{d?.app_version}</span>
      </div>
      <div>
        <h2 className="text-[16px] font-semibold tracking-tight">Check setup</h2>
        <p className="mt-0.5 text-[12px] leading-relaxed text-muted">What the widget can see on this computer. Use an AI tool for a minute, then compare the numbers with the tool's own usage screen.</p>
      </div>
      {!d ? (
        <p className="text-[12px] text-muted">Checking…</p>
      ) : (
        d.tools.map((t) => <ToolCard key={t.tool} t={t} d={d} />)
      )}
      <div className="flex gap-2">
        <button onClick={refresh} disabled={busy} className="flex-1 rounded-full bg-fg/10 py-2 text-[12.5px] font-medium hover:bg-fg/15 disabled:opacity-50">
          {busy ? "Refreshing…" : "Refresh now"}
        </button>
        <button onClick={copy} className="flex-1 rounded-full bg-fg/[0.05] py-2 text-[12.5px] text-muted hover:bg-fg/10">
          {copied ? "Copied ✓" : "Copy details"}
        </button>
      </div>
      <p className="text-[11px] leading-relaxed text-muted">Hover the pill to open this panel and move away to close it. {navigator.userAgent.includes("Mac") ? "Option" : "Alt"}+A toggles it from anywhere.</p>
    </div>
  );
}

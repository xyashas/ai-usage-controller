import { ToolMark } from "./ToolMark";
import { useEffect, useState } from "react";
import type { Diagnostics, HandoffOffer, Tool, ToolDiagnostics } from "../types";
import { api } from "../api";
import { toolName } from "./HandoffPreview";
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
    <div className="space-y-2 tile">
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
            <button className="ml-1 text-fg underline-offset-2 hover:underline" onClick={() => api.openUrl(cmp.url!)}>
              open
            </button>
          )}
        </li>
      </ul>
      {t.tool === "claude_code" && !gauge && !d.claude_web && <ClaudeConnect compact />}
    </div>
  );
}

export function CheckSetup({ onBack, onTestHandoff }: { onBack: () => void; onTestHandoff: (offer: HandoffOffer, target: Tool) => void }) {
  const [d, setD] = useState<Diagnostics | null>(null);
  const [offers, setOffers] = useState<HandoffOffer[] | null>(null);
  useEffect(() => {
    api.testHandoffOffers().then(setOffers, () => setOffers([]));
  }, []);
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
    <div className="flex flex-col gap-3 p-3">
      <div className="flex items-center justify-between">
        <button onClick={onBack} className="btn btn-ghost !pl-1.5">
          <BackIcon /> Back
        </button>
        <span className="text-[11px] text-muted">v{d?.app_version}</span>
      </div>
      <div>
        <h2 className="text-[17px] font-semibold tracking-tight">Check setup</h2>
        <p className="mt-0.5 text-[12px] leading-relaxed text-muted">What the widget can see on this computer. Use an AI tool for a minute, then compare the numbers with the tool's own usage screen.</p>
      </div>
      {!d ? (
        <p className="text-[12px] text-muted">Checking…</p>
      ) : (
        d.tools.map((t) => <ToolCard key={t.tool} t={t} d={d} />)
      )}
      <TestHandoff offers={offers} onTest={onTestHandoff} />
      <div className="flex gap-2">
        <button onClick={refresh} disabled={busy} className="btn flex-1">
          {busy ? "Refreshing…" : "Refresh now"}
        </button>
        <button onClick={copy} className="btn btn-ghost flex-1">
          {copied ? "Copied ✓" : "Copy details"}
        </button>
      </div>
      <p className="text-[11px] leading-relaxed text-muted">Hover the pill to open this panel and move away to close it. {navigator.userAgent.includes("Mac") ? "Option" : "Alt"}+A toggles it from anywhere.</p>
    </div>
  );
}

// Try the handoff on your latest session without waiting for a limit to reach 90%.
function TestHandoff({ offers, onTest }: { offers: HandoffOffer[] | null; onTest: (offer: HandoffOffer, target: Tool) => void }) {
  if (offers == null) return null;
  const usable = offers.filter((o) => o.targets.length > 0);
  return (
    <div className="space-y-2 tile">
      <p className="text-[13px] font-medium">Test the handoff</p>
      {usable.length ? (
        <>
          <p className="text-[11.5px] leading-relaxed text-muted">
            Uses your latest session from the last 24 hours, whatever your usage. You see the summary first and nothing runs until you press Continue.
          </p>
          {usable.map((o) => (
            <div key={o.source} className="flex flex-wrap gap-2">
              {o.targets.map((target) => (
                <button key={target} onClick={() => onTest(o, target)} className="btn grow !py-1.5 text-[12px]">
                  {toolName[o.source]} → {toolName[target]}
                </button>
              ))}
            </div>
          ))}
        </>
      ) : (
        <p className="text-[11.5px] leading-relaxed text-muted">
          {offers.length
            ? "Install Codex, Gemini CLI or Claude Code to have somewhere to hand off to."
            : "No Claude Code or Codex session on this computer in the last 24 hours. Use one in a project folder first (claude.ai chats can't be handed off)."}
        </p>
      )}
    </div>
  );
}

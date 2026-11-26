import type { Snapshot, ToolStatus } from "../types";
import { HeadroomBar } from "./HeadroomBar";
import { AlertBar } from "./AlertBar";
import { textColor, until, usd } from "../format";
import { api } from "../api";

type View = "settings" | "paywall" | "audit";

function SourceTag({ t }: { t: ToolStatus }) {
  const label = t.source === "live" ? "live" : t.source === "estimate" ? "estimate" : "learning";
  return (
    <span className="rounded-md bg-white/5 px-1.5 py-0.5 text-[10px] uppercase tracking-wide text-mute" title={t.live_error ?? (t.source === "live" ? "From the vendor's own gauge (best effort)" : t.source === "estimate" ? "Estimated from local logs" : "Learning your limits from usage history")}>
      {label}
    </span>
  );
}

function ActiveTool({ t, paid, onPaywall }: { t: ToolStatus; paid: boolean; onPaywall: () => void }) {
  const binding = [...t.windows].filter((w) => w.source !== "learning").sort((a, b) => a.headroom_pct - b.headroom_pct)[0];
  const pct = t.headroom_pct;
  return (
    <section className="space-y-2">
      <div className="flex items-baseline justify-between">
        <div className="flex items-center gap-2">
          <h2 className="text-[15px] font-semibold">{t.label}</h2>
          <SourceTag t={t} />
        </div>
        <span className={`tabular text-[22px] font-semibold ${textColor[t.color]}`}>{pct == null ? "—" : `${Math.round(pct)}%`}</span>
      </div>
      <HeadroomBar pct={pct} color={t.color} />
      <div className="flex justify-between text-[12px] text-mute">
        <span>
          {t.minutes_left != null ? (
            <span className="text-white/90">About {t.minutes_left} min left at this pace</span>
          ) : !paid && t.active ? (
            <button onClick={onPaywall} className="text-accent hover:underline">See time left →</button>
          ) : pct == null ? (
            "Learning your limits from usage history"
          ) : (
            `${binding?.label ?? ""} window left`
          )}
        </span>
        <span>{binding ? until(binding.resets_at) : ""}</span>
      </div>
      {t.windows.length > 1 && (
        <div className="flex gap-3 pt-0.5 text-[11px] text-mute">
          {t.windows.map((w) => (
            <span key={w.key} className="tabular">
              {w.label} {Math.round(w.headroom_pct)}% · {until(w.resets_at).replace("resets in ", "")}
            </span>
          ))}
        </div>
      )}
    </section>
  );
}

function ToolRow({ t, onPaywall }: { t: ToolStatus; onPaywall: () => void }) {
  const w = [...t.windows].sort((a, b) => a.headroom_pct - b.headroom_pct)[0];
  if (t.locked)
    return (
      <button onClick={onPaywall} className="flex w-full items-center gap-3 rounded-lg px-1 py-1.5 text-left hover:bg-white/5">
        <span className="w-24 text-[13px] text-white/80">{t.label}</span>
        <span className="flex-1 text-[11px] text-mute">Track all your tools with Pro</span>
        <span className="rounded-md bg-accent/20 px-1.5 py-0.5 text-[10px] font-semibold text-accent">PRO</span>
      </button>
    );
  return (
    <div className="flex items-center gap-3 px-1 py-1.5">
      <span className="w-24 text-[13px] text-white/80">{t.label}</span>
      <div className="flex-1">
        <HeadroomBar pct={t.headroom_pct} color={t.color} thin />
      </div>
      <span className={`tabular w-10 text-right text-[12px] ${textColor[t.color]}`}>{t.headroom_pct == null ? "—" : `${Math.round(t.headroom_pct)}%`}</span>
      <span className="tabular w-20 text-right text-[11px] text-mute">{w ? until(w.resets_at).replace("resets in ", "↻ ") : ""}</span>
    </div>
  );
}

export function Expanded({ snap, onView, refresh }: { snap: Snapshot; onView: (v: View) => void; refresh: () => void }) {
  const paid = snap.entitlements.tier !== "free";
  const active = snap.tools.find((t) => t.tool === snap.active_tool) ?? snap.tools.find((t) => t.detected);
  const others = snap.tools.filter((t) => t !== active && t.detected && t.enabled);
  const auditItem = snap.audit.find((a) => a.action === "downgrade" || a.action === "cancel" || a.action === "upgrade");
  const meter = snap.session_meter;

  return (
    <div className="fade-in flex flex-col gap-3.5 overflow-hidden rounded-[22px] bg-panel/95 p-4 ring-1 ring-line backdrop-blur-xl">
      {active ? (
        <ActiveTool t={active} paid={paid} onPaywall={() => onView("paywall")} />
      ) : (
        <p className="text-[13px] text-mute">No Claude Code, Codex or Gemini logs found yet. Run one of them once and they'll appear here.</p>
      )}

      {snap.alerts[0] && <AlertBar alert={snap.alerts[0]} onDone={refresh} />}

      {others.length > 0 && (
        <section className="-mx-1 border-t border-line pt-2">
          {others.map((t) => (
            <ToolRow key={t.tool} t={t} onPaywall={() => onView("paywall")} />
          ))}
        </section>
      )}

      <section className={`grid gap-2 border-t border-line pt-3 ${meter ? "grid-cols-3" : "grid-cols-2"}`}>
        {meter && <Stat label="This session" value={usd(meter.cost_usd)} hint={`${meter.minutes} min · ${meter.project_name ?? ""}`} />}
        <Stat label="Today" value={usd(snap.today.cost_usd)} hint={`${snap.today.sessions} sessions`} />
        <Stat
          label="Priciest"
          value={snap.today.top_session ? usd(snap.today.top_session.cost_usd) : paid ? "—" : "PRO"}
          hint={snap.today.top_session?.project_name ?? ""}
        />
      </section>

      {snap.nudge && (
        <div className="flex items-center gap-3 rounded-xl bg-accent/10 px-3 py-2.5 ring-1 ring-accent/25">
          <p className="flex-1 text-[12.5px] leading-snug">{snap.nudge.message}</p>
          <button onClick={async () => { await api.acceptNudge(); refresh(); }} className="shrink-0 rounded-lg bg-accent px-2.5 py-1 text-[12px] font-semibold text-black">
            Got it
          </button>
        </div>
      )}

      {auditItem && !snap.nudge && (
        <button onClick={() => onView("audit")} className="rounded-xl bg-white/5 px-3 py-2.5 text-left text-[12.5px] ring-1 ring-line hover:bg-white/10">
          <span className="text-mute">Monthly audit · </span>
          {auditItem.headline}
        </button>
      )}

      <div className="flex items-center justify-between border-t border-line pt-3 text-[12px]">
        {paid ? (
          <span className="text-mute">
            Saved this month <span className="tabular font-semibold text-ok">{usd(snap.savings_month_usd ?? 0, 0)}</span>
            <span className="text-mute/70"> est.</span>
          </span>
        ) : (
          <button onClick={() => onView("paywall")} className="font-medium text-accent">
            {snap.projected_saving_usd >= 1 ? `Save about ${usd(snap.projected_saving_usd, 0)}/mo →` : "Unlock savings →"}
          </button>
        )}
        <div className="flex gap-3 text-mute">
          {snap.entitlements.tier === "team" && (
            <button className="hover:text-white" onClick={() => api.openUrl("https://aiusage.dev/dashboard")}>Dashboard</button>
          )}
          <button className="hover:text-white" onClick={() => onView("settings")}>Settings</button>
        </div>
      </div>
    </div>
  );
}

function Stat({ label, value, hint }: { label: string; value: string; hint: string }) {
  return (
    <div className="min-w-0">
      <div className="text-[10.5px] uppercase tracking-wide text-mute">{label}</div>
      <div className="tabular text-[16px] font-semibold">{value}</div>
      <div className="truncate text-[11px] text-mute">{hint}</div>
    </div>
  );
}

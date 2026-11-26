import type { Snapshot, ToolStatus, WindowStatus } from "../types";
import type { View } from "../App";
import { UsageBar } from "./UsageBar";
import { AlertBar } from "./AlertBar";
import { ClaudeConnect } from "./ClaudeConnect";
import { countdown, textColor, usd, usedPct } from "../format";
import { api } from "../api";
import { GearIcon, PulseIcon } from "./icons";

const tight = (t: ToolStatus) => [...t.windows].filter((w) => w.source !== "learning").sort((a, b) => a.headroom_pct - b.headroom_pct)[0];
const pctText = (t: ToolStatus) => (t.color === "green" || t.color === "grey" ? "text-white" : textColor[t.color]);

function SourceChip({ t }: { t: ToolStatus }) {
  const live = t.source === "live" && !t.live_note;
  const label = t.live_note ? (t.reconnect ? "Signed out" : t.live_note) : live ? "Live" : t.needs_connect ? "Not connected" : t.source === "estimate" ? "Estimate" : "Learning";
  const tip = t.live_error ?? (live ? "Read from the vendor's own usage meter" : t.source === "estimate" ? "Estimated from local logs" : "Learning your limits from usage history");
  return (
    <span title={tip} className="flex items-center gap-1 rounded-full bg-white/[0.06] px-2 py-0.5 text-[10.5px] font-medium text-white/60">
      <span className={`h-1.5 w-1.5 rounded-full ${live ? "bg-ok" : "bg-white/30"}`} />
      {label}
    </span>
  );
}

function windowLine(w: WindowStatus) {
  const reset = countdown(w.resets_at);
  if (w.used_requests != null && w.allowance_requests != null) return `${w.used_requests.toLocaleString()} of ${w.allowance_requests.toLocaleString()} requests`;
  return reset ? `resets in ${reset}` : "starts when you next use it";
}

function Hero({ t, paid, onPaywall }: { t: ToolStatus; paid: boolean; onPaywall: () => void }) {
  const b = tight(t);
  // Not connected: the only numbers are estimates from Claude Code logs, which
  // miss claude.ai and desktop-app chats. Show them only if Claude Code was used.
  const estimateOnly = t.needs_connect;
  const used = estimateOnly && t.today_requests === 0 ? null : usedPct(t.headroom_pct);
  const others = t.windows.filter((w) => w !== b && w.source !== "learning");
  const reset = countdown(b?.resets_at ?? null);
  return (
    <section className="space-y-3">
      <div className="flex items-center justify-between">
        <div className="flex items-center gap-2">
          <h2 className="text-[15px] font-semibold tracking-tight">{t.label}</h2>
          <SourceChip t={t} />
        </div>
        {t.plan_name && <span className="text-[11.5px] text-white/40">{t.plan_name}</span>}
      </div>

      {used == null ? (
        <p className="text-[12.5px] leading-relaxed text-white/55">
          {t.needs_connect ? "Connect your account below to see your plan limits." : "Learning your limits from usage history. Numbers appear after a few sessions."}
        </p>
      ) : (
        <>
          <div className="flex items-end justify-between">
            <div className="flex items-baseline gap-1.5">
              <span className={`tabular text-[34px] font-semibold leading-none tracking-tight ${estimateOnly ? "text-white/45" : pctText(t)}`}>{Math.round(used)}%</span>
              <span className="text-[12.5px] text-white/45">{estimateOnly ? "estimated, Claude Code only" : `of ${b?.label.toLowerCase() ?? ""} limit used`}</span>
            </div>
            <span className="tabular pb-0.5 text-[11.5px] text-white/45">{reset ? `resets in ${reset}` : ""}</span>
          </div>
          <UsageBar used={used} color={t.color} size="lg" />
        </>
      )}

      {(others.length > 0 || t.minutes_left != null || (!paid && t.active)) && (
        <div className="space-y-1.5">
          {others.map((w) => (
            <div key={w.key} className="flex items-center gap-3 text-[11.5px]">
              <span className="w-20 text-white/50">{w.label}</span>
              <div className="flex-1">
                <UsageBar used={w.used_pct} color={w.headroom_pct <= 5 ? "red" : w.headroom_pct <= 20 ? "amber" : "green"} size="sm" />
              </div>
              <span className="tabular w-9 text-right text-white/80">{Math.round(w.used_pct)}%</span>
              <span className="tabular w-14 text-right text-white/35">{countdown(w.resets_at) ?? "—"}</span>
            </div>
          ))}
          {t.minutes_left != null ? (
            <p className="pt-0.5 text-[12px] text-white/75">At this pace you'll hit the limit in about {t.minutes_left} min.</p>
          ) : !paid && t.active && !estimateOnly ? (
            <button onClick={onPaywall} className="pt-0.5 text-[12px] text-accent hover:underline">See how long you have left at this pace →</button>
          ) : null}
        </div>
      )}
    </section>
  );
}

function ToolRow({ t, onPaywall }: { t: ToolStatus; onPaywall: () => void }) {
  const w = tight(t);
  const used = usedPct(t.headroom_pct);
  if (t.locked)
    return (
      <button onClick={onPaywall} className="flex w-full items-center gap-3 rounded-xl px-2 py-2 text-left hover:bg-white/[0.04]">
        <span className="w-16 text-[12.5px] text-white/75">{t.label}</span>
        <span className="flex-1 text-[11.5px] text-white/40">Track every tool with Pro</span>
        <span className="rounded-full bg-accent/15 px-2 py-0.5 text-[10px] font-semibold text-accent">PRO</span>
      </button>
    );
  return (
    <div className="flex items-center gap-3 px-2 py-2" title={w ? windowLine(w) : undefined}>
      <span className="w-16 text-[12.5px] text-white/75">{t.label}</span>
      <div className="flex-1">
        <UsageBar used={used} color={t.color} size="sm" />
      </div>
      <span className={`tabular w-9 text-right text-[12px] font-medium ${pctText(t)}`}>{used == null ? "—" : `${Math.round(used)}%`}</span>
      <span className="tabular w-14 text-right text-[11px] text-white/35">{countdown(w?.resets_at ?? null) ?? "—"}</span>
    </div>
  );
}

export function Expanded({ snap, onView, refresh }: { snap: Snapshot; onView: (v: View) => void; refresh: () => void }) {
  const paid = snap.entitlements.tier !== "free";
  const active = snap.tools.find((t) => t.tool === snap.active_tool && t.enabled) ?? snap.tools.find((t) => t.detected && t.enabled);
  const others = snap.tools.filter((t) => t !== active && t.detected && t.enabled);
  const claude = snap.tools.find((t) => t.tool === "claude_code" && t.enabled);
  const auditItem = snap.audit.find((a) => a.action === "downgrade" || a.action === "cancel" || a.action === "upgrade");
  const meter = snap.session_meter;
  const hasActivity = meter != null || snap.today.cost_usd > 0.004;

  return (
    <div className="flex flex-col gap-3.5 p-5">
      {active ? (
        <Hero t={active} paid={paid} onPaywall={() => onView("paywall")} />
      ) : (
        <p className="text-[13px] leading-relaxed text-white/60">No Claude, Codex or Gemini usage found yet. Connect your Claude account, or run one of the command-line tools once.</p>
      )}

      {claude?.live_note && claude !== active && !claude.reconnect && <p className="text-xs text-white/55">Claude ? {claude.live_note}</p>}
      {claude && (claude.reconnect || (claude.needs_connect && !claude.live_note)) && <ClaudeConnect tool={claude} onChanged={refresh} />}

      {snap.alerts[0] && <AlertBar alert={snap.alerts[0]} onDone={refresh} />}

      {others.length > 0 && (
        <section className="-mx-2 border-t border-white/[0.06] pt-2">
          {others.map((t) => (
            <ToolRow key={t.tool} t={t} onPaywall={() => onView("paywall")} />
          ))}
        </section>
      )}

      {hasActivity && (
        <section className={`grid gap-2 border-t border-white/[0.06] pt-4 ${meter ? "grid-cols-3" : "grid-cols-2"}`}>
          {meter && <Stat label="This session" value={usd(meter.cost_usd)} hint={`${meter.minutes} min${meter.project_name ? ` · ${meter.project_name}` : ""}`} />}
          <Stat label="Today" value={usd(snap.today.cost_usd)} hint={`${snap.today.sessions} session${snap.today.sessions === 1 ? "" : "s"}`} />
          <Stat label="Priciest" value={snap.today.top_session ? usd(snap.today.top_session.cost_usd) : paid ? "—" : "Pro"} hint={snap.today.top_session?.project_name ?? ""} />
        </section>
      )}

      {snap.nudge && (
        <div className="flex items-center gap-3 rounded-2xl bg-accent/10 px-3.5 py-3 ring-1 ring-accent/20">
          <p className="flex-1 text-[12.5px] leading-snug text-white/85">{snap.nudge.message}</p>
          <button onClick={async () => { await api.acceptNudge(); refresh(); }} className="shrink-0 rounded-full bg-accent px-3 py-1 text-[12px] font-semibold text-black">
            Got it
          </button>
        </div>
      )}

      {auditItem && !snap.nudge && (
        <button onClick={() => onView("audit")} className="rounded-2xl bg-white/[0.04] px-3.5 py-3 text-left text-[12.5px] leading-snug ring-1 ring-white/[0.06] hover:bg-white/[0.07]">
          <span className="text-white/45">Monthly audit · </span>
          {auditItem.headline}
        </button>
      )}

      <footer className="sticky bottom-0 -mx-5 -mb-5 flex items-center justify-between border-t border-white/[0.06] bg-black px-5 pb-4 pt-3 text-[12px]">
        {paid ? (
          <span className="text-white/45">
            Saved this month <span className="tabular font-semibold text-ok">{usd(snap.savings_month_usd ?? 0, 0)}</span> est.
          </span>
        ) : (
          <button onClick={() => onView("paywall")} className="font-medium text-accent hover:underline">
            {snap.projected_saving_usd >= 1 ? `Save about ${usd(snap.projected_saving_usd, 0)}/mo →` : "Unlock Pro →"}
          </button>
        )}
        <div className="flex items-center gap-1 text-white/45">
          {snap.entitlements.tier === "team" && (
            <button className="rounded-full px-2 py-1 hover:bg-white/[0.06] hover:text-white" onClick={() => api.openUrl("https://aiusage.dev/dashboard")}>Dashboard</button>
          )}
          <button title="Check setup" aria-label="Check setup" className="rounded-full p-1.5 hover:bg-white/[0.06] hover:text-white" onClick={() => onView("setup")}>
            <PulseIcon />
          </button>
          <button title="Settings" aria-label="Settings" className="rounded-full p-1.5 hover:bg-white/[0.06] hover:text-white" onClick={() => onView("settings")}>
            <GearIcon />
          </button>
        </div>
      </footer>
    </div>
  );
}

function Stat({ label, value, hint }: { label: string; value: string; hint: string }) {
  return (
    <div className="min-w-0">
      <div className="text-[10.5px] font-medium uppercase tracking-wider text-white/35">{label}</div>
      <div className="tabular mt-0.5 text-[16px] font-semibold">{value}</div>
      <div className="truncate text-[11px] text-white/40">{hint}</div>
    </div>
  );
}

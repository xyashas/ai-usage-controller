import { useEffect, useState } from "react";
import type { Alert, HandoffOffer, RepoStatus, Snapshot, Tool, ToolStatus, WindowStatus } from "../types";
import type { View } from "../App";
import { ToolMark } from "./ToolMark";
import { UsageHistory } from "./UsageHistory";
import { ClaudeConnect } from "./ClaudeConnect";
import { TopBar } from "./TopBar";
import { countdown, usd, usedPct } from "../format";
import { api } from "../api";
import { alertTint, stateColor, tint, toolName, warnTint } from "../theme";
import { ArrowIcon } from "./icons";

/** The window shown first: the 5-hour limit when there is one, else the tightest. */
export function primaryWindow(t: ToolStatus): WindowStatus | undefined {
  const known = t.windows.filter((w) => w.source !== "learning");
  return known.find((w) => w.key === "five_hour") ?? [...known].sort((a, b) => a.headroom_pct - b.headroom_pct)[0];
}

const colorOf = (w: WindowStatus) => (w.headroom_pct <= 5 ? "red" : w.headroom_pct <= 20 ? "amber" : "green") as "red" | "amber" | "green";

function Bar({ used, color, thin = false }: { used: number; color: string; thin?: boolean }) {
  const w = Math.max(used > 0 ? 2 : 0, Math.min(100, used));
  return (
    <div className={`w-full overflow-hidden rounded-full ${thin ? "h-1" : "h-1.5"}`} style={{ background: "rgba(0,0,0,0.1)" }}>
      <div className="h-full rounded-full transition-[width] duration-700 ease-out" style={{ width: `${w}%`, background: color }} />
    </div>
  );
}

function LiveChip({ t }: { t: ToolStatus }) {
  const live = t.source === "live" && !t.live_note;
  const label = t.live_note ? (t.reconnect ? "Signed out" : t.live_note) : live ? "Live" : t.source === "estimate" ? "Estimate" : "Learning";
  const tip = t.live_error ?? (live ? "Read from the vendor's own usage meter" : t.source === "estimate" ? "Estimated from local logs" : "Learning your limits from usage history");
  return (
    <span title={tip} className="flex items-center gap-1 text-[10.5px] font-medium opacity-60">
      <span className={`h-1.5 w-1.5 rounded-full ${live ? "live-dot" : ""}`} style={{ background: live ? "#1f9d55" : "currentColor" }} />
      {label}
    </span>
  );
}

function ToolCard({ t, snap, wide, paid, onView, refresh }: { t: ToolStatus; snap: Snapshot; wide: boolean; paid: boolean; onView: (v: View) => void; refresh: () => void }) {
  const c = tint[t.tool];
  const head = (
    <div className="flex items-center justify-between gap-2">
      <span title={t.plan_name ?? undefined} className="flex min-w-0 items-center gap-1.5 text-[12.5px] font-semibold">
        <ToolMark tool={t.tool} />
        <span className="truncate">{t.tool === "claude_code" ? "Claude" : t.label}</span>
      </span>
      {!t.locked && !t.needs_connect && <LiveChip t={t} />}
    </div>
  );

  if (t.locked) {
    return (
      <button onClick={() => onView("paywall")} className="card fade-in flex min-h-[150px] flex-col justify-between text-left" style={{ background: c.bg, color: c.ink }}>
        {head}
        <div className="space-y-2">
          <p className="text-[12px] leading-snug opacity-70">Track {t.label} too</p>
          <span className="btn btn-ink !px-3 !py-1 text-[11px]">Pro</span>
        </div>
      </button>
    );
  }

  const w = primaryWindow(t);
  const others = t.windows.filter((x) => x !== w && x.source !== "learning");
  const connect = t.needs_connect || t.reconnect;
  const used = w ? usedPct(w.headroom_pct) : null;
  const reset = countdown(w?.resets_at ?? null);

  return (
    <section className="card fade-in flex flex-col gap-2.5" style={{ background: c.bg, color: c.ink }}>
      {head}
      {connect && t.tool === "claude_code" ? (
        <div className="flex flex-1 flex-col justify-end gap-2">
          <p className="text-[12px] leading-snug opacity-70">{t.reconnect ? "Your claude.ai sign-in expired." : "Sign in once to see your 5-hour and weekly limits."}</p>
          <ClaudeConnect tool={t} onChanged={refresh} inline />
        </div>
      ) : used == null || !w ? (
        <div className="flex flex-1 flex-col justify-end gap-1">
          <p className="text-[12px] leading-snug opacity-70">Learning your limits. A few sessions and the meter fills in.</p>
          <button className="self-start text-[11.5px] font-semibold underline-offset-2 hover:underline" onClick={() => onView("setup")}>Check sources</button>
        </div>
      ) : (
        <>
          <div>
            <div className="flex items-baseline gap-1">
              <span className="tabular text-[36px] font-semibold leading-none tracking-[-0.03em]" style={{ color: w.headroom_pct <= 20 ? stateColor(t.tool, colorOf(w)) : undefined }}>
                {Math.round(used)}
              </span>
              <span className="text-[16px] font-semibold opacity-50">%</span>
            </div>
            <p className="mt-1 text-[11.5px] leading-tight opacity-60">
              {w.label} limit used{wide && reset ? ` · resets in ${reset}` : ""}
            </p>
          </div>
          <div className="space-y-1.5">
            <Bar used={used} color={stateColor(t.tool, colorOf(w))} />
            {!wide && <p className="tabular text-[11px] opacity-60">{reset ? `Resets in ${reset}` : w.used_requests != null ? `${w.used_requests} of ${w.allowance_requests} requests` : "Starts on next use"}</p>}
          </div>
          {others.map((o) => (
            <div key={o.key} className="space-y-1">
              <div className="tabular flex items-baseline justify-between text-[11px]">
                <span className="opacity-60">{o.label}</span>
                <span><span className="font-semibold">{Math.round(o.used_pct)}%</span><span className="opacity-50">{countdown(o.resets_at) ? ` · ${countdown(o.resets_at)}` : ""}</span></span>
              </div>
              <Bar used={o.used_pct} color={stateColor(t.tool, colorOf(o))} thin />
            </div>
          ))}
          {t.minutes_left != null ? (
            <p className="text-[11.5px] font-medium leading-snug">{t.minutes_left} min left at this pace</p>
          ) : !paid && t.active ? (
            <button onClick={() => onView("paywall")} className="self-start text-[11.5px] font-medium opacity-70 hover:opacity-100">Time left at this pace →</button>
          ) : null}
          {wide && t.tool === "claude_code" && snap.gauge_history.length > 3 && <UsageHistory points={snap.gauge_history} end={Date.parse(snap.generated_at)} color={c.bar} />}
        </>
      )}
    </section>
  );
}

/** "Continue in …" buttons on a tinted alert tile. */
function Destinations({ targets, onPick }: { targets: Tool[]; onPick: (t: Tool) => void }) {
  return (
    <div className="flex flex-wrap gap-1.5">
      {targets.map((t) => (
        <button key={t} onClick={() => onPick(t)} className="btn btn-ink grow !py-2">
          <ToolMark tool={t} />
          {toolName[t]}
          <ArrowIcon />
        </button>
      ))}
    </div>
  );
}

function AlertCard({ alert, snap, offer, cloud, onHandoff, onRepoHandoff, refresh }: {
  alert: Alert; snap: Snapshot; offer?: HandoffOffer; cloud?: RepoStatus; refresh: () => void;
  onHandoff: (o: HandoffOffer, t: Tool) => void; onRepoHandoff: (r: RepoStatus, t: Tool) => void;
}) {
  const [busy, setBusy] = useState(false);
  const [msg, setMsg] = useState<string | null>(null);
  const red = alert.level === "red";
  const c = red ? alertTint : warnTint;
  const tool = snap.tools.find((t) => t.tool === alert.tool);
  const w = tool && primaryWindow(tool);
  const reset = countdown(w?.resets_at ?? null);
  const cloudTargets = (["claude_code", "codex", "gemini"] as Tool[]).filter((t) => t !== alert.tool);

  async function act() {
    setBusy(true);
    try {
      if (alert.action.kind === "dismiss") await api.dismiss(alert.id);
      else {
        const r = await api.alertAction(alert);
        if (r && r !== "ok") setMsg(r);
      }
      refresh();
    } catch (e) {
      setMsg(String(e));
    } finally {
      setBusy(false);
    }
  }

  return (
    <section className="card fade-in space-y-3" style={{ background: c.bg, color: c.ink }} role="alert">
      <div className="flex items-start gap-2.5">
        <span className={`mt-1.5 h-2 w-2 shrink-0 rounded-full ${red ? "live-dot" : ""}`} style={{ background: c.bar }} />
        <div className="min-w-0 flex-1">
          <p className="text-[13.5px] font-semibold leading-snug">{msg ?? alert.message}</p>
          {alert.kind === "low_headroom" && reset && <p className="tabular mt-0.5 text-[11.5px] opacity-60">Resets in {reset}</p>}
        </div>
        {!offer && !cloud && (
          <button disabled={busy} onClick={act} className="btn btn-ink shrink-0 !px-3 !py-1.5 text-[12px]">{alert.action.label}</button>
        )}
      </div>
      {offer ? (
        <div className="space-y-1.5">
          <p className="text-[11px] font-medium opacity-60">Carry on where you left off in</p>
          <Destinations targets={offer.targets} onPick={(t) => onHandoff(offer, t)} />
        </div>
      ) : cloud ? (
        <div className="space-y-1.5">
          <p className="text-[11px] font-medium opacity-60">Continue {cloud.name}{cloud.latest?.branch ? ` (${cloud.latest.branch})` : ""} in</p>
          <Destinations targets={cloudTargets} onPick={(t) => onRepoHandoff(cloud, t)} />
        </div>
      ) : null}
    </section>
  );
}

function Stat({ label, value, hint }: { label: string; value: string; hint: string }) {
  return (
    <div className="min-w-0">
      <div className="eyebrow">{label}</div>
      <div className="tabular mt-1 text-[19px] font-semibold tracking-tight">{value}</div>
      <div className="truncate text-[11px] text-muted">{hint || " "}</div>
    </div>
  );
}

export function Expanded({ snap, onView, refresh, onHandoff, onRepoHandoff }: {
  onHandoff: (offer: HandoffOffer, target: Tool) => void; onRepoHandoff: (r: RepoStatus, t: Tool) => void;
  snap: Snapshot; onView: (v: View) => void; refresh: () => void;
}) {
  const paid = snap.entitlements.tier !== "free";
  const tools = snap.tools.filter((t) => t.detected && t.enabled);
  // The one you're using goes first.
  tools.sort((a, b) => Number(b.tool === snap.active_tool) - Number(a.tool === snap.active_tool));
  const auditItem = snap.audit.find((a) => a.action === "downgrade" || a.action === "cancel" || a.action === "upgrade");
  const meter = snap.session_meter;
  const hasActivity = meter != null || snap.today.cost_usd > 0.004;

  const alert = snap.alerts[0];
  const lowTool = alert?.kind === "low_headroom" ? alert.tool : null;
  const alertOffer = snap.handoffs.find((o) => o.source === lowTool && o.targets.length);
  const otherOffers = snap.handoffs.filter((o) => o !== alertOffer && o.targets.length);

  // Near a limit with no local session to hand off (you were working in the
  // cloud): offer the newest repo handoff instead.
  const [cloud, setCloud] = useState<RepoStatus | undefined>();
  const needCloud = !!lowTool && !alertOffer;
  useEffect(() => {
    if (!needCloud) return setCloud(undefined);
    let live = true;
    api.listRepos(false).then((repos) => {
      const fresh = repos.filter((r) => r.set_up && r.latest && Date.now() / 1000 - r.latest.updated_at < 12 * 3600)
        .sort((a, b) => b.latest!.updated_at - a.latest!.updated_at)[0];
      if (live) setCloud(fresh);
    }, () => {});
    return () => { live = false; };
  }, [needCloud]);

  return (
    <div className="flex flex-col gap-2.5 p-3">
      <TopBar tab="panel" onView={onView} />

      {alert && <AlertCard alert={alert} snap={snap} offer={alertOffer} cloud={cloud} onHandoff={onHandoff} onRepoHandoff={onRepoHandoff} refresh={refresh} />}

      {otherOffers.map((o) => (
        <section key={o.source} className="tile fade-in space-y-2.5">
          <p className="text-[12.5px] font-medium">{toolName[o.source]} is close to its limit. Continue the session in</p>
          <div className="flex flex-wrap gap-1.5">
            {o.targets.map((t) => (
              <button key={t} className="btn grow" onClick={() => onHandoff(o, t)}><ToolMark tool={t} />{toolName[t]}</button>
            ))}
          </div>
        </section>
      ))}

      {tools.length ? (
        <div className="grid gap-2.5" style={{ gridTemplateColumns: `repeat(${Math.min(3, tools.length)}, minmax(0, 1fr))` }}>
          {tools.map((t) => (
            <ToolCard key={t.tool} t={t} snap={snap} wide={tools.length === 1} paid={paid} onView={onView} refresh={refresh} />
          ))}
        </div>
      ) : (
        <section className="tile space-y-2">
          <p className="text-[14px] font-semibold">Ready for your first session</p>
          <p className="text-[12px] text-muted">No AI tools found yet. Check where the widget looks for your usage.</p>
          <button className="btn" onClick={() => onView("setup")}>Check setup</button>
        </section>
      )}

      {hasActivity && (
        <section className={`tile grid gap-3 ${meter ? "grid-cols-3" : "grid-cols-2"}`}>
          {meter && <Stat label="This session" value={usd(meter.cost_usd)} hint={`${meter.minutes} min${meter.project_name ? ` · ${meter.project_name}` : ""}`} />}
          <Stat label="Today" value={usd(snap.today.cost_usd)} hint={`${snap.today.sessions} session${snap.today.sessions === 1 ? "" : "s"}`} />
          <Stat label="Priciest" value={snap.today.top_session ? usd(snap.today.top_session.cost_usd) : paid ? "—" : "Pro"} hint={snap.today.top_session?.project_name ?? ""} />
        </section>
      )}

      {snap.nudge && (
        <section className="tile flex items-center gap-3">
          <p className="flex-1 text-[12.5px] leading-snug">{snap.nudge.message}</p>
          <button onClick={async () => { await api.acceptNudge(); refresh(); }} className="btn btn-primary shrink-0 !py-1.5">Got it</button>
        </section>
      )}

      {auditItem && !snap.nudge && (
        <button onClick={() => onView("audit")} className="tile text-left text-[12.5px] leading-snug transition-colors hover:bg-[#1b1b1e]">
          <span className="eyebrow mb-1 block">Monthly audit</span>
          {auditItem.headline}
        </button>
      )}

      <footer className="flex items-center justify-between px-1.5 pb-0.5 pt-0.5 text-[11.5px] text-muted">
        {paid ? (
          <span>Saved this month <span className="tabular font-semibold text-fg">{usd(snap.savings_month_usd ?? 0, 0)}</span> est.</span>
        ) : (
          <button onClick={() => onView("paywall")} className="font-medium text-fg hover:underline">
            {snap.projected_saving_usd >= 1 ? `Save about ${usd(snap.projected_saving_usd, 0)}/mo with Pro →` : "Unlock Pro →"}
          </button>
        )}
        {snap.entitlements.tier === "team" && (
          <button className="hover:text-fg" onClick={() => api.openUrl("https://aiusage.dev/dashboard")}>Team dashboard →</button>
        )}
      </footer>
    </div>
  );
}

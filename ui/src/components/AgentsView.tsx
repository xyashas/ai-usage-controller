import { useEffect, useRef, useState } from "react";
import type { View } from "../App";
import type { AgentRun, AgentState, Dest, DestOption, DispatchPlan, RepoStatus, Settings, Snapshot, Started, Tool } from "../types";
import { Approvals } from "./Approvals";
import { preferredDest, rememberDest } from "../prefs";
import { GithubConnect, GithubMark } from "./GithubConnect";
import { api } from "../api";
import { tint, toolName } from "../theme";
import { usd } from "../format";
import { TopBar } from "./TopBar";
import { ToolMark } from "./ToolMark";
import { BranchIcon, CloseIcon, MicIcon } from "./icons";
import { PolishButton, PolishDiffCard, usePromptPolish } from "./PromptPolish";
import { CiRuns } from "./CiRuns";
import { Watches } from "./Watches";

const mac = navigator.userAgent.includes("Mac");

export const stateLabel: Record<AgentState, string> = {
  starting: "Starting",
  working: "Working",
  idle: "Your turn",
  done: "Done",
  needs_approval: "Needs your OK",
  no_activity: "Not started",
  opened: "Opened",
  queued: "Queued",
};

function since(iso: string) {
  const m = Math.max(0, Math.round((Date.now() - Date.parse(iso)) / 60000));
  return m < 60 ? `${m} min` : `${Math.floor(m / 60)} h ${m % 60} min`;
}

export const clock = (iso: string) => new Date(iso).toLocaleTimeString([], { hour: "numeric", minute: "2-digit" });

export function StateDot({ state }: { state: AgentState }) {
  const color = state === "working" ? "#4ade80" : state === "needs_approval" || state === "idle" ? "#fbbf24" : state === "done" ? "#7cb8ff" : "rgba(255,255,255,0.35)";
  const pulse = state === "working" || state === "needs_approval";
  return <span className={`inline-block h-1.5 w-1.5 shrink-0 rounded-full ${pulse ? "live-dot" : ""}`} style={{ background: color }} />;
}

const secs = (s: number) => (s < 90 ? `${Math.round(s)} s` : `${Math.round(s / 60)} min`);

function Verify({ a, open, onToggle, onRun }: { a: AgentRun; open: boolean; onToggle: () => void; onRun: () => void }) {
  if (a.verify === "running") return <span className="flex items-center gap-1 text-muted"><span className="animate-spin inline-block h-2.5 w-2.5 rounded-full border border-current border-t-transparent" />Testing…</span>;
  if (a.verify === "passed") return <button onClick={onToggle} className="font-medium text-ok hover:underline">✓ Tests pass{a.verify_secs ? ` · ${secs(a.verify_secs)}` : ""}</button>;
  if (a.verify === "failed" || a.verify === "timed_out" || a.verify === "error") {
    const label = a.verify === "failed" ? "Tests fail" : a.verify === "timed_out" ? "Tests took too long" : "Couldn't run tests";
    return <button onClick={onToggle} className="font-medium text-bad hover:underline">✗ {label}{open ? "" : " · show"}</button>;
  }
  if ((a.state === "done" || a.state === "idle") && a.dest !== "web") return <button onClick={onRun} className="text-muted opacity-0 transition-opacity hover:text-fg group-hover:opacity-100">Run tests</button>;
  return null;
}

function AgentRow({ a, refresh, roomiest, handedTo }: { a: AgentRun; refresh: () => void; roomiest: (not: Tool) => Tool | null; handedTo?: Tool }) {
  const c = tint[a.tool];
  const [out, setOut] = useState<string | null>(null);
  const [err, setErr] = useState("");
  const other = a.state === "queued" ? roomiest(a.tool) : null;
  const act = (f: () => Promise<unknown>) => { setErr(""); f().then(refresh, (e) => setErr(String(e))); };
  async function toggleOutput() {
    if (out != null) return setOut(null);
    setOut((await api.verifyOutput(a.id).catch(() => null)) ?? "The output is kept until the widget restarts. Run the tests again to see it.");
  }
  return (
    <div className="group rounded-2xl px-3 py-2.5 transition-colors hover:bg-white/[0.04]">
      <div className="flex items-center gap-3">
        <span className="flex h-8 w-8 shrink-0 items-center justify-center rounded-xl" style={{ background: c.bg, color: c.ink }} title={toolName[a.tool]}>
          <ToolMark tool={a.tool} />
        </span>
        <div className="min-w-0 flex-1">
          <p className="truncate text-[12.5px] font-medium">{a.text}</p>
          <p className="flex items-center gap-1 truncate text-[11px] text-muted">
            <span>{a.repo_name}</span>
            {a.branch && <><span>·</span><BranchIcon /><span className="truncate">{a.branch}</span></>}
            {a.dest && a.dest !== "terminal" && <span>· {a.dest === "web" ? "web" : "app"}</span>}
            {handedTo && <span className="text-accent">· Handed to {toolName[handedTo]}</span>}
          </p>
        </div>
        <div className="shrink-0 text-right">
          <p className="flex items-center justify-end gap-1.5 text-[11.5px] font-medium"><StateDot state={a.state} />{stateLabel[a.state]}</p>
          <p className="tabular text-[10.5px] text-muted">
            {a.state === "queued" && a.queued_until ? `Sends at ${clock(a.queued_until)}` : <>{since(a.started_at)}{a.cost_usd > 0 ? ` · ${usd(a.cost_usd)}` : ""}</>}
          </p>
        </div>
        <button className="icon-btn -mr-1 opacity-0 transition-opacity group-hover:opacity-100 focus:opacity-100" aria-label={a.state === "queued" ? "Cancel" : "Remove from list"} title={a.state === "queued" ? "Cancel" : "Remove from list"} onClick={() => api.dismissAgent(a.id).then(refresh)}>
          <CloseIcon />
        </button>
      </div>
      {(a.state === "queued" || a.verify || a.state === "done" || a.state === "idle" || err) && (
        <div className="ml-11 mt-1 flex items-center gap-2 text-[11px]">
          {a.state === "queued" ? (
            <>
              <button className="btn !px-2.5 !py-1 text-[11px]" onClick={() => act(() => api.runQueued(a.id))}>Send now</button>
              {other && <button className="btn btn-ghost !px-2.5 !py-1 text-[11px]" onClick={() => act(() => api.runQueued(a.id, other))}><ToolMark tool={other} />Move to {toolName[other]}</button>}
            </>
          ) : (
            <Verify a={a} open={out != null} onToggle={toggleOutput} onRun={() => act(() => api.verifyTask(a.id))} />
          )}
          {err && <span className="truncate text-bad">{err}</span>}
        </div>
      )}
      {out != null && <pre className="ml-11 mt-2 max-h-40 select-text overflow-auto whitespace-pre-wrap rounded-xl bg-black/40 p-2.5 font-mono text-[10.5px] leading-relaxed text-muted">{out}</pre>}
    </div>
  );
}

/** Off until you turn it on: it adds hooks to Claude Code's and Codex's settings. */
function StatusSetup({ onDone }: { onDone: () => void }) {
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState("");
  async function on() {
    setBusy(true);
    setErr("");
    try {
      const r = await api.setAgentStatus(true);
      if (r.codex_busy) setErr("Codex already notifies another program, so only Claude Code reports here. Codex is still followed from its logs.");
      onDone();
    } catch (e) { setErr(String(e)); }
    finally { setBusy(false); }
  }
  return (
    <section className="tile fade-in space-y-2.5">
      <div>
        <p className="text-[13px] font-semibold">Know the moment an agent needs you</p>
        <p className="mt-1 text-[11.5px] leading-relaxed text-muted">
          Claude Code and Codex tell the widget when they finish or need your OK, and you can allow Claude Code's requests from here. Adds a hook to <code className="text-fg">~/.claude/settings.json</code> and <code className="text-fg">~/.codex/config.toml</code>. Only the folder and the event are recorded, never your prompts.
        </p>
      </div>
      <button disabled={busy} className="btn btn-primary w-full" onClick={on}>{busy ? "Turning on…" : "Turn on agent status"}</button>
      {err && <p className="text-[11.5px] text-muted">{err}</p>}
    </section>
  );
}

type RunItem = { text: string; tool: Tool; dest: DestOption; queue: boolean };

export function AgentsView({ snap, onView, refresh, prefill, onPrefilled }: { snap: Snapshot; onView: (v: View) => void; refresh: () => void; prefill?: string | null; onPrefilled?: () => void }) {
  const [repos, setRepos] = useState<RepoStatus[] | null>(null);
  const [repo, setRepo] = useState("");
  const [text, setText] = useState("");
  const [plan, setPlan] = useState<DispatchPlan | null>(null);
  const [picks, setPicks] = useState<Record<number, Tool>>({});
  const [destPicks, setDestPicks] = useState<Record<number, Dest>>({});
  const [queuePicks, setQueuePicks] = useState<Record<number, boolean>>({});
  const [settings, setSettings] = useState<Settings | null>(null);
  // App and web tasks go one at a time: each needs a paste from the clipboard.
  const [queue, setQueue] = useState<{ repo: string; tasks: RunItem[] } | null>(null);
  const [github, setGithub] = useState(false);
  const [worktrees, setWorktrees] = useState<boolean | null>(null);
  const [busy, setBusy] = useState(false);
  const [result, setResult] = useState<Started[] | null>(null);
  const [err, setErr] = useState("");
  const box = useRef<HTMLTextAreaElement>(null);
  const polish = usePromptPolish(text, repo || null);

  // Text handed over from Ask ("Fix it with an agent", "Make it a task").
  useEffect(() => { if (prefill) { setText(prefill); onPrefilled?.(); } }, [prefill]);

  const loadRepos = () => api.listRepos(false).then((r) => { setRepos(r); setRepo((cur) => cur || r[0]?.id || ""); }, () => setRepos([]));
  useEffect(() => {
    loadRepos();
    api.settings().then(setSettings, () => {});
    api.focus().finally(() => box.current?.focus({ preventScroll: true }));
  }, []);

  // Re-plan as you type.
  useEffect(() => {
    if (!text.trim()) { setPlan(null); return; }
    const id = setTimeout(() => api.planDispatch(text, repo || null).then((p) => { setPlan(p); setPicks({}); setDestPicks({}); setQueuePicks({}); }, () => {}), 220);
    return () => clearTimeout(id);
  }, [text, repo]);

  const ready = new Set(plan?.rooms.filter((r) => r.ready).map((r) => r.tool));
  const tasks = (plan?.tasks ?? []).map((t, i) => {
    const tool = picks[i] ?? t.tool;
    const options: DestOption[] = tool ? plan?.dests[tool] ?? [] : [];
    const dest = options.find((o) => o.dest === destPicks[i]) ?? (tool ? preferredDest(tool, options) : undefined);
    // At its limit: offer to hold the task until the limit resets (on by default).
    const room = plan?.rooms.find((r) => r.tool === tool);
    const full = !!room?.resets_at && (room.used_pct ?? 0) >= 98;
    const queue = full && (queuePicks[i] ?? true);
    return { ...t, tool, options, dest, full, queue, resetsAt: room?.resets_at ?? null };
  });
  const runnable = tasks.length > 0 && tasks.every((t) => t.tool && ready.has(t.tool) && t.dest) && !!repo;
  const current = repos?.find((r) => r.id === repo);
  const local = current?.local ?? true;
  const anyLocal = tasks.some((t) => t.dest && t.dest.dest !== "web");
  const useWorktrees = local && (worktrees ?? tasks.length > 1);

  function cycle(i: number) {
    const order = (["claude_code", "codex", "gemini"] as Tool[]).filter((t) => ready.has(t));
    if (!order.length) return;
    const now = tasks[i].tool;
    setPicks({ ...picks, [i]: order[(order.indexOf(now as Tool) + 1) % order.length] });
    setDestPicks({ ...destPicks, [i]: undefined as unknown as Dest });
  }

  function cycleDest(i: number) {
    const t = tasks[i];
    if (t.options.length < 2 || !t.dest) return;
    const next = t.options[(t.options.findIndex((o) => o.dest === t.dest!.dest) + 1) % t.options.length];
    setDestPicks({ ...destPicks, [i]: next.dest });
  }

  async function dictate() {
    setErr("");
    await api.focus();
    box.current?.focus();
    try { await api.startDictation(); }
    catch (e) { setErr(String(e)); }
  }

  async function send(repoId: string, items: RunItem[], wt: boolean) {
    const started = await api.runDispatch(repoId, items.map(({ text, tool, dest, queue }) => ({ text, tool, dest: dest.dest, queue })), wt);
    setResult((prev) => [...(prev ?? []), ...started]);
    started.forEach((s) => { if (!s.error) rememberDest(s.tool, s.dest); });
    refresh();
    return started;
  }

  async function run() {
    if (!runnable || busy) return;
    setBusy(true);
    setErr("");
    setResult([]);
    try {
      const items: RunItem[] = tasks.map((t) => ({ text: t.text, tool: t.tool!, dest: t.dest!, queue: t.queue }));
      // Terminals all start at once (queued ones are only recorded); the first
      // app/web task too. The rest go one by one, each with the clipboard to itself.
      const terminal = items.filter((t) => t.dest.dest === "terminal" || t.queue);
      const pasted = items.filter((t) => t.dest.dest !== "terminal" && !t.queue);
      const first = [...terminal, ...pasted.slice(0, 1)];
      const started = await send(repo, first, useWorktrees);
      if (started.some((s) => !s.error)) { setText(""); setPlan(null); setWorktrees(null); }
      // With auto-send on, each app/web task goes as soon as the one before
      // was pasted in. If one couldn't be, stop and let you continue by hand.
      let rest = pasted.slice(1);
      let ok = started[started.length - 1]?.delivered ?? false;
      while (ok && rest.length) {
        const [s] = await send(repo, rest.slice(0, 1), useWorktrees);
        ok = !!s?.delivered;
        rest = rest.slice(1);
      }
      setQueue(rest.length ? { repo, tasks: rest } : null);
    } catch (e) { setErr(String(e)); }
    finally { setBusy(false); }
  }

  async function next() {
    if (!queue || busy) return;
    setBusy(true);
    try {
      await send(queue.repo, queue.tasks.slice(0, 1), useWorktrees);
      setQueue(queue.tasks.length > 1 ? { ...queue, tasks: queue.tasks.slice(1) } : null);
    } catch (e) { setErr(String(e)); }
    finally { setBusy(false); }
  }

  const live = snap.agents;
  // Where a queued task could go right now: the other tool with the most room.
  const roomiest = (not: Tool) => snap.tools.filter((t) => t.enabled && t.detected && t.tool !== not && (t.headroom_pct ?? 100) > 10)
    .sort((a, b) => (b.headroom_pct ?? 0) - (a.headroom_pct ?? 0))[0]?.tool ?? null;

  return (
    <div className="flex flex-col gap-2.5 p-3">
      <TopBar tab="agents" onView={onView} />
      <Approvals items={snap.approvals} refresh={refresh} />

      <section className="tile !p-0">
        <textarea
          ref={box}
          rows={3}
          value={text}
          onFocus={() => api.focus()}
          onChange={(e) => { setText(e.target.value); setResult(null); }}
          onKeyDown={(e) => { if (e.key === "Enter" && (e.metaKey || e.ctrlKey)) { e.preventDefault(); run(); } }}
          spellCheck={false}
          placeholder={"What should your agents do?\nTry: Have Codex fix the failing checkout test, and Claude update the docs."}
          className="block w-full resize-none bg-transparent px-4 pb-2 pt-3.5 text-[14px] leading-relaxed outline-none placeholder:text-[13px]"
          style={{ outline: "none" }}
        />
        <div className="flex items-center gap-2 px-2.5 pb-2.5">
          <button className="icon-btn" title={mac ? "Dictate (or press Fn twice)" : "Dictate (Win+H)"} aria-label="Dictate" onClick={dictate}><MicIcon /></button>
          {repos && repos.length > 0 ? (
            <select
              aria-label="Repository"
              value={repo}
              onChange={(e) => setRepo(e.target.value)}
              className="max-w-[170px] truncate rounded-full bg-white/[0.07] px-3 py-1.5 text-[12px] outline-none"
            >
              {repos.some((r) => r.local) && (
                <optgroup label="On this computer">
                  {repos.filter((r) => r.local).map((r) => <option key={r.id} value={r.id}>{r.name}</option>)}
                </optgroup>
              )}
              {repos.some((r) => !r.local) && (
                <optgroup label="On GitHub (cloud)">
                  {repos.filter((r) => !r.local).map((r) => <option key={r.id} value={r.id}>{r.name}</option>)}
                </optgroup>
              )}
            </select>
          ) : null}
          {!repos?.some((r) => !r.local) && (
            <button className="icon-btn" title="Connect GitHub to use all your repos" aria-label="Connect GitHub" onClick={() => setGithub(!github)}><GithubMark /></button>
          )}
          {local && anyLocal && <button
            role="switch"
            aria-checked={useWorktrees}
            onClick={() => setWorktrees(!useWorktrees)}
            title="Run each task on its own branch in a separate folder, so agents don't edit the same files"
            className="flex items-center gap-1.5 rounded-full px-2 py-1.5 text-[11.5px] text-muted hover:text-fg"
          >
            <span className={`flex h-[14px] w-6 items-center rounded-full px-[2px] transition-colors ${useWorktrees ? "bg-[#34c759]" : "bg-white/15"}`}>
              <span className={`h-[10px] w-[10px] rounded-full bg-white transition-transform ${useWorktrees ? "translate-x-[10px]" : ""}`} />
            </span>
            Worktrees
          </button>}
          <div className="ml-auto flex items-center gap-1.5">
          <PolishButton disabled={!text.trim() || busy} busy={polish.busy} onClick={polish.polish} />
          <button disabled={!runnable || busy} onClick={run} className="btn btn-primary !py-1.5">
            {busy ? "Sending…" : tasks.length && tasks.every((t) => t.queue) ? (tasks.length > 1 ? `Queue ${tasks.length} tasks` : "Queue") : tasks.length > 1 ? `Run ${tasks.length} tasks` : "Run"}
            <span className="text-[10.5px] font-medium opacity-50">{mac ? "⌘↵" : "Ctrl↵"}</span>
          </button>
          </div>
        </div>
      </section>
      {polish.error && <p className="px-1 text-[11.5px] text-bad">{polish.error}</p>}
      {polish.draft && <PolishDiffCard original={polish.draft.original} polished={polish.draft.polished} onAccept={(next) => { setText(next); polish.clear(); }} onKeep={polish.clear} />}

      {tasks.length > 0 && (
        <section className="fade-in space-y-1.5">
          {tasks.map((t, i) => {
            const c = t.tool ? tint[t.tool] : null;
            const blocked = !t.tool || !ready.has(t.tool);
            return (
              <div key={i} className="flex items-center gap-2.5 px-1">
                <button
                  onClick={() => cycle(i)}
                  title="Change tool"
                  className="flex shrink-0 items-center gap-1.5 rounded-full px-2.5 py-1 text-[11.5px] font-semibold transition-transform active:scale-95"
                  style={c ? { background: c.bg, color: c.ink } : { background: "rgba(255,255,255,0.08)" }}
                >
                  {t.tool && <ToolMark tool={t.tool} />}
                  {t.tool ? toolName[t.tool] : "No tool"}
                </button>
                <span className="min-w-0 flex-1 truncate text-[12.5px]" title={picks[i] ? "Your pick" : t.reason}>{t.text}</span>
                {t.full && !blocked && (
                  <button
                    role="switch"
                    aria-checked={t.queue}
                    onClick={() => setQueuePicks({ ...queuePicks, [i]: !t.queue })}
                    title={t.queue ? "Held until the limit resets. Click to send now anyway." : "Click to hold it until the limit resets"}
                    className={`shrink-0 rounded-full px-2.5 py-1 text-[11px] font-medium transition-colors ${t.queue ? "bg-[#fbbf24]/15 text-warn" : "bg-white/[0.07] text-muted hover:text-fg"}`}
                  >
                    {t.queue ? `Queued · ${clock(t.resetsAt!)}` : "Send now"}
                  </button>
                )}
                {blocked ? (
                  <span className="shrink-0 text-[10.5px] text-bad">{t.reason}</span>
                ) : t.dest && (
                  <button
                    onClick={() => cycleDest(i)}
                    disabled={t.options.length < 2}
                    title={t.options.length > 1 ? `Open in: ${t.options.map((o) => o.label).join(", ")}. Click to change.` : undefined}
                    className="shrink-0 rounded-full bg-white/[0.07] px-2.5 py-1 text-[11px] font-medium text-muted transition-colors hover:bg-white/[0.12] hover:text-fg"
                  >
                    {t.dest.label}{t.options.length > 1 ? " ▾" : ""}
                  </button>
                )}
              </div>
            );
          })}
          <p className="px-1 pt-0.5 text-[11px] text-mute">
            {!local
              ? `Runs in the cloud on ${current?.name}.`
              : useWorktrees
              ? `Each task gets its own branch from ${current?.current_branch || "the current commit"}, in ${current?.root.split(/[\\/]/).pop() || "the repo"}.agents/ next to the repo. Uncommitted changes stay where they are.`
              : `Runs in ${current?.name || "the repo"} on ${current?.current_branch || "the current branch"}.`}
            {local && " Terminal tasks start together; app and web tasks are sent one after another."}
            {tasks.some((t) => t.dest && t.dest.dest !== "terminal" && !t.queue) && " Keep your hands off the keyboard for a few seconds while they're sent."}
            {tasks.some((t) => t.queue) && " Queued tasks go out when the limit resets."}
          </p>
        </section>
      )}

      {github && <GithubConnect onChange={() => { setGithub(false); loadRepos(); }} />}

      {result && result.length > 0 && (
        <section className="tile fade-in space-y-1.5 !py-3">
          {result.map((r, i) => (
            <p key={i} className={`text-[12px] leading-snug ${r.error ? "text-bad" : "text-muted"}`}>
              {r.error ? `✗ ${r.text}: ${r.error}` : `✓ ${toolName[r.tool]}: ${r.message ?? "started"}${r.branch ? ` · ${r.branch}` : ""}`}
            </p>
          ))}
          {queue && (
            <div className="flex items-center gap-2 border-t border-white/[0.06] pt-2.5">
              <p className="min-w-0 flex-1 truncate text-[12px]">Next: <span className="text-fg">{queue.tasks[0].text}</span></p>
              <button disabled={busy} className="btn btn-primary shrink-0 !py-1.5 text-[12px]" onClick={next}>
                Copy & open {queue.tasks[0].dest.label}{queue.tasks.length > 1 ? ` (${queue.tasks.length} left)` : ""}
              </button>
            </div>
          )}
        </section>
      )}
      {err && <p className="px-1 text-[12px] text-bad">{err}</p>}

      {settings && !settings.agent_status && <StatusSetup onDone={() => api.settings().then(setSettings)} />}

      <div className="flex items-center justify-between px-1 pt-1">
        <p className="eyebrow">Today</p>
        {live.length > 0 && <p className="tabular text-[11px] text-mute">{usd(live.reduce((s, a) => s + a.cost_usd, 0))} est.</p>}
      </div>
      {live.length ? (
        <section className="tile !p-1">
          {live.map((a) => <AgentRow key={a.id} a={a} refresh={refresh} roomiest={roomiest} handedTo={snap.autopilot_moves?.find((m) => m.task_id === a.id)?.to} />)}
        </section>
      ) : (
        <p className="px-1 pb-1 text-[12px] leading-relaxed text-muted">
          Tasks you run show up here with what each agent is doing: working, waiting for you, or done with the tests run. {mac ? "⌥⇧A" : "Alt+Shift+A"} opens this from anywhere.
        </p>
      )}
      <CiRuns />
      <Watches />
    </div>
  );
}

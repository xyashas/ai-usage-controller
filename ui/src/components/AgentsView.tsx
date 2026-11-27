import { useEffect, useRef, useState } from "react";
import type { View } from "../App";
import type { AgentRun, DispatchPlan, RepoStatus, Snapshot, Started, Tool } from "../types";
import { api } from "../api";
import { tint, toolName } from "../theme";
import { usd } from "../format";
import { TopBar } from "./TopBar";
import { ToolMark } from "./ToolMark";
import { BranchIcon, CloseIcon, MicIcon } from "./icons";

const mac = navigator.userAgent.includes("Mac");

export const stateLabel: Record<AgentRun["state"], string> = {
  starting: "Starting",
  working: "Working",
  idle: "Your turn",
  no_activity: "Not started",
};

function since(iso: string) {
  const m = Math.max(0, Math.round((Date.now() - Date.parse(iso)) / 60000));
  return m < 60 ? `${m} min` : `${Math.floor(m / 60)} h ${m % 60} min`;
}

export function StateDot({ state }: { state: AgentRun["state"] }) {
  const color = state === "working" ? "#4ade80" : state === "idle" ? "#fbbf24" : "rgba(255,255,255,0.35)";
  return <span className={`inline-block h-1.5 w-1.5 shrink-0 rounded-full ${state === "working" ? "live-dot" : ""}`} style={{ background: color }} />;
}

function AgentRow({ a, refresh }: { a: AgentRun; refresh: () => void }) {
  const c = tint[a.tool];
  return (
    <div className="group flex items-center gap-3 rounded-2xl px-3 py-2.5 transition-colors hover:bg-white/[0.04]">
      <span className="flex h-8 w-8 shrink-0 items-center justify-center rounded-xl" style={{ background: c.bg, color: c.ink }} title={toolName[a.tool]}>
        <ToolMark tool={a.tool} />
      </span>
      <div className="min-w-0 flex-1">
        <p className="truncate text-[12.5px] font-medium">{a.text}</p>
        <p className="flex items-center gap-1 truncate text-[11px] text-muted">
          <span>{a.repo_name}</span>
          {a.branch && <><span>·</span><BranchIcon /><span className="truncate">{a.branch}</span></>}
        </p>
      </div>
      <div className="shrink-0 text-right">
        <p className="flex items-center justify-end gap-1.5 text-[11.5px] font-medium"><StateDot state={a.state} />{stateLabel[a.state]}</p>
        <p className="tabular text-[10.5px] text-muted">{since(a.started_at)}{a.cost_usd > 0 ? ` · ${usd(a.cost_usd)}` : ""}</p>
      </div>
      <button className="icon-btn -mr-1 opacity-0 transition-opacity group-hover:opacity-100 focus:opacity-100" aria-label="Remove from list" title="Remove from list" onClick={() => api.dismissAgent(a.id).then(refresh)}>
        <CloseIcon />
      </button>
    </div>
  );
}

export function AgentsView({ snap, onView, refresh }: { snap: Snapshot; onView: (v: View) => void; refresh: () => void }) {
  const [repos, setRepos] = useState<RepoStatus[] | null>(null);
  const [repo, setRepo] = useState("");
  const [text, setText] = useState("");
  const [plan, setPlan] = useState<DispatchPlan | null>(null);
  const [picks, setPicks] = useState<Record<number, Tool>>({});
  const [worktrees, setWorktrees] = useState<boolean | null>(null);
  const [busy, setBusy] = useState(false);
  const [result, setResult] = useState<Started[] | null>(null);
  const [err, setErr] = useState("");
  const box = useRef<HTMLTextAreaElement>(null);

  useEffect(() => {
    api.listRepos(false).then((r) => { setRepos(r); setRepo((cur) => cur || r[0]?.root || ""); }, () => setRepos([]));
    api.focus().finally(() => box.current?.focus({ preventScroll: true }));
  }, []);

  // Re-plan as you type.
  useEffect(() => {
    if (!text.trim()) { setPlan(null); return; }
    const id = setTimeout(() => api.planDispatch(text).then((p) => { setPlan(p); setPicks({}); }, () => {}), 220);
    return () => clearTimeout(id);
  }, [text]);

  const tasks = (plan?.tasks ?? []).map((t, i) => ({ ...t, tool: picks[i] ?? t.tool }));
  const ready = new Set(plan?.rooms.filter((r) => r.ready).map((r) => r.tool));
  const runnable = tasks.length > 0 && tasks.every((t) => t.tool && ready.has(t.tool)) && !!repo;
  const useWorktrees = worktrees ?? tasks.length > 1;
  const current = repos?.find((r) => r.root === repo);

  function cycle(i: number) {
    const order = (["claude_code", "codex", "gemini"] as Tool[]).filter((t) => ready.has(t));
    if (!order.length) return;
    const now = tasks[i].tool;
    setPicks({ ...picks, [i]: order[(order.indexOf(now as Tool) + 1) % order.length] });
  }

  async function dictate() {
    setErr("");
    await api.focus();
    box.current?.focus();
    try { await api.startDictation(); }
    catch (e) { setErr(String(e)); }
  }

  async function run() {
    if (!runnable || busy) return;
    setBusy(true);
    setErr("");
    try {
      const started = await api.runDispatch(repo, tasks.map((t) => ({ text: t.text, tool: t.tool! })), useWorktrees);
      setResult(started);
      if (started.some((s) => !s.error)) { setText(""); setPlan(null); setWorktrees(null); }
      refresh();
    } catch (e) { setErr(String(e)); }
    finally { setBusy(false); }
  }

  const live = snap.agents;

  return (
    <div className="flex flex-col gap-2.5 p-3">
      <TopBar tab="agents" onView={onView} />

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
              {repos.map((r) => <option key={r.root} value={r.root}>{r.name}</option>)}
            </select>
          ) : (
            <button className="btn btn-ghost !py-1.5 text-[12px]" onClick={() => onView("handoffs")}>Add a repo</button>
          )}
          <button
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
          </button>
          <button disabled={!runnable || busy} onClick={run} className="btn btn-primary ml-auto !py-1.5">
            {busy ? "Starting…" : tasks.length > 1 ? `Run ${tasks.length} tasks` : "Run"}
            <span className="text-[10.5px] font-medium opacity-50">{mac ? "⌘↵" : "Ctrl↵"}</span>
          </button>
        </div>
      </section>

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
                <span className="min-w-0 flex-1 truncate text-[12.5px]">{t.text}</span>
                <span className={`shrink-0 text-[10.5px] ${blocked ? "text-bad" : "text-mute"}`}>{picks[i] ? "Your pick" : t.reason}</span>
              </div>
            );
          })}
          <p className="px-1 pt-0.5 text-[11px] text-mute">
            {useWorktrees
              ? `Each task gets its own branch from ${current?.current_branch || "the current commit"}, in ${current?.root.split(/[\\/]/).pop() || "the repo"}.agents/ next to the repo. Uncommitted changes stay where they are.`
              : `Runs in ${current?.name || "the repo"} on ${current?.current_branch || "the current branch"}.`}
            {" "}Each opens in its own terminal.
          </p>
        </section>
      )}

      {result && (
        <section className="tile fade-in space-y-1 !py-3">
          {result.map((r, i) => (
            <p key={i} className={`text-[12px] ${r.error ? "text-bad" : "text-muted"}`}>
              {r.error ? `✗ ${r.text}: ${r.error}` : `✓ ${toolName[r.tool]} started${r.branch ? ` on ${r.branch}` : ""}`}
            </p>
          ))}
        </section>
      )}
      {err && <p className="px-1 text-[12px] text-bad">{err}</p>}

      <div className="flex items-center justify-between px-1 pt-1">
        <p className="eyebrow">Today</p>
        {live.length > 0 && <p className="tabular text-[11px] text-mute">{usd(live.reduce((s, a) => s + a.cost_usd, 0))} est.</p>}
      </div>
      {live.length ? (
        <section className="tile !p-1">
          {live.map((a) => <AgentRow key={a.id} a={a} refresh={refresh} />)}
        </section>
      ) : (
        <p className="px-1 pb-1 text-[12px] leading-relaxed text-muted">
          Tasks you run show up here with what each agent is doing: working, or waiting for you. {mac ? "⌥⇧A" : "Alt+Shift+A"} opens this from anywhere.
        </p>
      )}
    </div>
  );
}

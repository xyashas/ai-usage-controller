import { useCallback, useEffect, useMemo, useState } from "react";
import type { View } from "../App";
import type { HandoffOffer, HandoffSession, RepoStatus, SetupResult, Tool } from "../types";
import { api } from "../api";
import { tint, toolName } from "../theme";
import { TopBar } from "./TopBar";
import { ToolMark } from "./ToolMark";
import { BranchIcon, CloseIcon, PlusIcon, RefreshIcon } from "./icons";
import { GithubConnect, GithubMark } from "./GithubConnect";

const ALL: Tool[] = ["claude_code", "codex", "gemini"];

function since(seconds: number) {
  const s = Math.max(0, Date.now() / 1000 - seconds);
  if (s < 90) return "just now";
  if (s < 3600) return `${Math.round(s / 60)} min ago`;
  if (s < 86400) return `${Math.round(s / 3600)} h ago`;
  return `${Math.round(s / 86400)} d ago`;
}

/** Where cloud agents pick a handoff up: the auto section says "(cloud)". */
const fromCloud = (r: RepoStatus) => /\(cloud\)/.test(r.latest?.content ?? "");

function SessionPicker({ sessions, offers, onPick }: {
  sessions: HandoffSession[];
  offers: HandoffOffer[];
  onPick: (o: HandoffOffer, t: Tool) => void;
}) {
  const [project, setProject] = useState<string>("all");
  const [selectedKey, setSelectedKey] = useState<string | null>(null);

  const projects = useMemo(() => {
    const seen = new Set<string>();
    const list: string[] = [];
    for (const s of sessions) {
      if (!seen.has(s.project)) {
        seen.add(s.project);
        list.push(s.project);
      }
    }
    return list;
  }, [sessions]);

  const filtered = useMemo(
    () => (project === "all" ? sessions : sessions.filter((s) => s.project === project)),
    [sessions, project]
  );

  // Default-select the newest visible session so target buttons are 1 click away.
  const activeKey = filtered.some((s) => `${s.source}:${s.session}` === selectedKey)
    ? selectedKey
    : filtered[0] ? `${filtered[0].source}:${filtered[0].session}` : null;

  if (sessions.length > 0) {
    return (
      <div className="space-y-2">
        <div className="flex items-center justify-between px-1">
          <p className="eyebrow">Choose a chat to hand off</p>
          <span className="text-[11px] text-mute">{filtered.length} session{filtered.length === 1 ? "" : "s"}</span>
        </div>

        {projects.length > 1 && (
          <div className="flex flex-wrap gap-1 px-0.5" role="group" aria-label="Filter by project">
            <button
              className={`rounded-full px-2.5 py-1 text-[11px] font-medium transition-colors ${project === "all" ? "bg-white/15 text-fg" : "bg-white/[0.05] text-muted hover:text-fg"}`}
              aria-pressed={project === "all"}
              onClick={() => setProject("all")}
            >
              All projects
            </button>
            {projects.map((p) => (
              <button
                key={p}
                className={`rounded-full px-2.5 py-1 text-[11px] font-medium transition-colors ${project === p ? "bg-white/15 text-fg" : "bg-white/[0.05] text-muted hover:text-fg"}`}
                aria-pressed={project === p}
                onClick={() => setProject(p)}
              >
                {p}
              </button>
            ))}
          </div>
        )}

        <div className="space-y-1.5">
          {filtered.slice(0, 8).map((s) => {
            const key = `${s.source}:${s.session}`;
            const active = key === activeKey;
            const c = tint[s.source];
            return (
              <section
                key={key}
                className={`fade-in rounded-2xl transition-colors ${active ? "card space-y-2.5" : "tile !p-3 cursor-pointer hover:bg-white/[0.06]"}`}
                style={active ? { background: c.bg, color: c.ink } : undefined}
                onClick={() => { if (!active) setSelectedKey(key); }}
              >
                <div className="flex items-center gap-1.5 text-[12px] font-semibold">
                  <ToolMark tool={s.source} />
                  <span className="truncate">{s.project}</span>
                  <span className={`ml-auto shrink-0 text-[10.5px] font-normal ${active ? "opacity-65" : "text-mute"}`}>
                    {toolName[s.source]} · {since(s.updated_at)}
                  </span>
                </div>
                <p className={`line-clamp-2 text-[12px] leading-snug ${active ? "opacity-90" : "text-muted"}`}>
                  {s.title}
                </p>
                {active && (
                  <div className="flex flex-wrap gap-1.5 pt-0.5">
                    {s.targets.map((t) => (
                      <button
                        key={t}
                        className="btn btn-ink grow !py-2"
                        onClick={(e) => {
                          e.stopPropagation();
                          onPick({ source: s.source, session: s.session, targets: s.targets }, t);
                        }}
                      >
                        <ToolMark tool={t} />Continue in {toolName[t]}
                      </button>
                    ))}
                  </div>
                )}
              </section>
            );
          })}
        </div>
      </div>
    );
  }

  if (!offers.length) return null;
  return (
    <div className="space-y-2">
      <p className="eyebrow px-1">Latest sessions</p>
      {offers.map((o) => {
        const c = tint[o.source];
        return (
          <section key={o.source} className="card fade-in space-y-2.5" style={{ background: c.bg, color: c.ink }}>
            <div className="flex items-center gap-1.5 text-[12.5px] font-semibold"><ToolMark tool={o.source} />Latest {toolName[o.source]} session</div>
            <div className="flex flex-wrap gap-1.5">
              {o.targets.map((t) => (
                <button key={t} className="btn btn-ink grow !py-2" onClick={() => onPick(o, t)}>
                  <ToolMark tool={t} />Continue in {toolName[t]}
                </button>
              ))}
            </div>
          </section>
        );
      })}
    </div>
  );
}

function SetupSheet({ repo, onDone, onCancel }: { repo: RepoStatus; onDone: (r: SetupResult) => void; onCancel: () => void }) {
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState("");
  async function run(commit: boolean) {
    setBusy(true);
    setErr("");
    try { onDone(await api.setupRepo(repo.root, commit)); }
    catch (e) { setErr(String(e)); setBusy(false); }
  }
  return (
    <div className="fade-in space-y-2.5 border-t border-white/[0.06] pt-3">
      <p className="text-[12px] leading-relaxed text-muted">
        Adds a small <code className="text-fg">.aiuc/</code> folder, a handoff rule to <code className="text-fg">AGENTS.md</code> and <code className="text-fg">CLAUDE.md</code>, and a Claude Code hook that keeps <code className="text-fg">.aiuc/handoff.md</code> current after every turn, locally and in the cloud. Codex and Gemini follow the rule.
      </p>
      <div className="flex gap-1.5">
        <button disabled={busy} className="btn btn-primary grow" onClick={() => run(true)}>{busy ? "Setting up…" : `Set up and commit on ${repo.current_branch || "this branch"}`}</button>
        <button disabled={busy} className="btn" onClick={() => run(false)}>Files only</button>
        <button disabled={busy} className="icon-btn" aria-label="Cancel" title="Cancel" onClick={onCancel}><CloseIcon /></button>
      </div>
      <p className="text-[11px] text-mute">Nothing is pushed. Push the commit so cloud agents get it.</p>
      {err && <p className="text-[11.5px] text-bad">{err}</p>}
    </div>
  );
}

function RepoTile({ repo, onPick, onChanged }: { repo: RepoStatus; onPick: (r: RepoStatus, t: Tool) => void; onChanged: () => void }) {
  const [setup, setSetup] = useState(false);
  const [result, setResult] = useState<SetupResult | null>(null);
  const l = repo.latest;
  return (
    <section className="tile fade-in space-y-3">
      <div className="flex items-start gap-2">
        <div className="min-w-0 flex-1">
          <p className="flex items-center gap-1.5 truncate text-[13.5px] font-semibold tracking-tight">
            {repo.github && <span className="text-muted" title="Also on GitHub, so cloud agents can work on it"><GithubMark /></span>}
            <span className="truncate">{repo.name}</span>
          </p>
          <p className="truncate text-[11px] text-mute" title={repo.root}>{repo.root}</p>
          {l ? (
            <p className="mt-0.5 flex items-center gap-1 truncate text-[11.5px] text-muted">
              <BranchIcon /><span className="truncate">{l.branch || repo.current_branch}</span>
              <span>· {since(l.updated_at)}{fromCloud(repo) ? " · cloud" : ""}{l.origin === "working tree" ? " · not committed" : ""}</span>
            </p>
          ) : (
            <p className="mt-0.5 text-[11.5px] text-muted">{repo.set_up ? "Set up. The first handoff appears after an AI session here." : "Not set up for handoffs"}</p>
          )}
        </div>
        {repo.set_up ? (
          <span className="mt-0.5 rounded-full bg-[#4ade80]/15 px-2 py-0.5 text-[10.5px] font-semibold text-ok">Ready</span>
        ) : !setup && !result ? (
          <button className="btn !px-3 !py-1.5 text-[12px]" onClick={() => setSetup(true)}>Set up</button>
        ) : null}
      </div>

      {setup && !result && <SetupSheet repo={repo} onCancel={() => setSetup(false)} onDone={(r) => { setResult(r); onChanged(); }} />}
      {result && (
        <p className="fade-in text-[12px] leading-relaxed text-muted">
          {result.changed.length === 0 ? "Already set up." : result.committed ? `Committed ${result.changed.length} files on ${repo.current_branch}. Push it so cloud agents pick it up.` : `Added ${result.changed.length} files. Commit and push them when you're ready.`}
        </p>
      )}

      {repo.set_up && l && (
        <div className="flex gap-1.5">
          {ALL.map((t) => (
            <button key={t} className="btn grow !px-2 !py-1.5 text-[12px]" onClick={() => onPick(repo, t)} title={`Continue in ${toolName[t]}`}>
              <ToolMark tool={t} />{toolName[t]}
            </button>
          ))}
        </div>
      )}
    </section>
  );
}

function RemoteRow({ repo, onCloned }: { repo: RepoStatus; onCloned: () => void }) {
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState("");
  async function clone() {
    setBusy(true);
    setErr("");
    try { if (await api.cloneRepo(repo.name)) onCloned(); }
    catch (e) { setErr(String(e)); }
    finally { setBusy(false); }
  }
  return (
    <div className="flex items-center gap-2.5 rounded-2xl px-3 py-2 hover:bg-white/[0.04]">
      <span className="text-muted"><GithubMark /></span>
      <div className="min-w-0 flex-1">
        <p className="truncate text-[12.5px] font-medium">{repo.name}</p>
        {err && <p className="text-[11px] text-bad">{err}</p>}
      </div>
      <button className="btn btn-ghost !px-2 !py-1 text-[11.5px]" onClick={() => repo.github && api.openUrl(repo.github)}>Open</button>
      <button disabled={busy} className="btn !px-3 !py-1 text-[11.5px]" onClick={clone}>{busy ? "Cloning…" : "Clone"}</button>
    </div>
  );
}

export function HandoffHub({ onView, onHandoff, onRepoHandoff }: {
  onView: (v: View) => void; onHandoff: (o: HandoffOffer, t: Tool) => void; onRepoHandoff: (r: RepoStatus, t: Tool) => void;
}) {
  const [sessions, setSessions] = useState<HandoffSession[]>([]);
  const [offers, setOffers] = useState<HandoffOffer[]>([]);
  const [repos, setRepos] = useState<RepoStatus[] | null>(null);
  const [fetching, setFetching] = useState(false);
  const [err, setErr] = useState("");
  const [allRemote, setAllRemote] = useState(false);

  const load = useCallback(async (fetch: boolean) => {
    if (fetch) setFetching(true);
    try { setRepos(await api.listRepos(fetch)); }
    catch (e) { setErr(String(e)); setRepos([]); }
    finally { setFetching(false); }
  }, []);

  const loadSessions = useCallback(() => {
    api.listHandoffSessions().then((list) => setSessions(list.filter((x) => x.targets.length)), () => {});
    api.testHandoffOffers().then((o) => setOffers(o.filter((x) => x.targets.length)), () => {});
  }, []);

  useEffect(() => {
    loadSessions();
    // Show what's here at once, then check GitHub for handoffs pushed from the cloud.
    load(false).then(() => load(true));
  }, [load, loadSessions]);

  async function add() {
    setErr("");
    try { if (await api.addRepo()) await load(false); }
    catch (e) { setErr(String(e)); }
  }

  return (
    <div className="flex flex-col gap-2.5 p-3">
      <TopBar tab="handoffs" onView={onView} />
      <p className="px-1 text-[12px] leading-relaxed text-muted">
        Pick the project and chat you want to carry over to another AI with its goal, changed files, and what's left.
      </p>

      <SessionPicker sessions={sessions} offers={offers} onPick={onHandoff} />

      <div className="flex items-center justify-between px-1 pt-1">
        <p className="eyebrow">Repos on this computer</p>
        <div className="flex items-center gap-0.5">
          <button className="icon-btn" title="Refresh sessions and GitHub handoffs" aria-label="Refresh sessions and GitHub handoffs" disabled={fetching} onClick={() => { loadSessions(); load(true); }}><RefreshIcon spin={fetching} /></button>
          <button className="icon-btn" title="Add a repo" aria-label="Add a repo" onClick={add}><PlusIcon /></button>
        </div>
      </div>
      {repos == null ? (
        <p className="px-1 py-3 text-[12px] text-muted">Looking for your repos…</p>
      ) : repos.some((r) => r.local) ? (
        repos.filter((r) => r.local).map((r) => <RepoTile key={r.id} repo={r} onPick={onRepoHandoff} onChanged={() => load(false)} />)
      ) : (
        <section className="tile space-y-2 text-center">
          <p className="text-[12.5px] text-muted">Repos you work in with Claude Code or Codex show up here.</p>
          <button className="btn" onClick={add}><PlusIcon />Add a repo</button>
        </section>
      )}
      {repos && (repos.some((r) => !r.local) ? (
        <>
          <p className="eyebrow px-1 pt-2">On GitHub</p>
          <div className="tile !p-1.5">
            {repos.filter((r) => !r.local).slice(0, allRemote ? 999 : 6).map((r) => (
              <RemoteRow key={r.id} repo={r} onCloned={() => load(false)} />
            ))}
            {!allRemote && repos.filter((r) => !r.local).length > 6 && (
              <button className="w-full py-1.5 text-center text-[11.5px] text-muted hover:text-fg" onClick={() => setAllRemote(true)}>
                Show all {repos.filter((r) => !r.local).length} repos
              </button>
            )}
          </div>
        </>
      ) : <GithubConnect onChange={() => load(false)} />)}
      {err && <p className="px-1 text-[11.5px] text-bad">{err}</p>}
    </div>
  );
}

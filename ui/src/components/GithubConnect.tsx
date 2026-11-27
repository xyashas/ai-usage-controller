import { useEffect, useRef, useState } from "react";
import type { DeviceCode, GithubStatus } from "../types";
import { api } from "../api";

const TOKEN_URL = "https://github.com/settings/tokens/new?scopes=repo&description=AI%20Usage%20Controller";

export function GithubMark() {
  return (
    <svg width="15" height="15" viewBox="0 0 16 16" fill="currentColor" aria-hidden>
      <path d="M8 0C3.58 0 0 3.58 0 8c0 3.54 2.29 6.53 5.47 7.59.4.07.55-.17.55-.38v-1.33c-2.23.48-2.7-1.07-2.7-1.07-.36-.92-.89-1.17-.89-1.17-.73-.5.06-.49.06-.49.8.06 1.23.83 1.23.83.72 1.22 1.87.87 2.33.66.07-.52.28-.87.5-1.07-1.78-.2-3.65-.89-3.65-3.95 0-.87.31-1.59.82-2.15-.08-.2-.36-1.02.08-2.12 0 0 .67-.21 2.2.82a7.6 7.6 0 0 1 4 0c1.53-1.04 2.2-.82 2.2-.82.44 1.1.16 1.92.08 2.12.51.56.82 1.27.82 2.15 0 3.07-1.87 3.75-3.65 3.95.29.25.54.73.54 1.48v2.2c0 .21.15.46.55.38A8 8 0 0 0 16 8c0-4.42-3.58-8-8-8Z" />
    </svg>
  );
}

/** Connect GitHub so every repo there can get tasks and handoffs. */
export function GithubConnect({ onChange, compact = false }: { onChange?: () => void; compact?: boolean }) {
  const [status, setStatus] = useState<GithubStatus | null>(null);
  const [device, setDevice] = useState<DeviceCode | null>(null);
  const [showToken, setShowToken] = useState(false);
  const [token, setToken] = useState("");
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState("");
  const polling = useRef<number | undefined>(undefined);

  const load = () => api.githubStatus().then(setStatus, () => {});
  useEffect(() => { load(); return () => clearTimeout(polling.current); }, []);

  async function run(f: () => Promise<unknown>) {
    setBusy(true);
    setErr("");
    try { await f(); await load(); onChange?.(); }
    catch (e) { setErr(String(e)); }
    finally { setBusy(false); }
  }

  async function signIn() {
    setErr("");
    try {
      const code = await api.githubDeviceStart();
      setDevice(code);
      const poll = async () => {
        try {
          const login = await api.githubDevicePoll(code.device_code);
          if (login) { setDevice(null); await load(); onChange?.(); return; }
          polling.current = window.setTimeout(poll, code.interval * 1000);
        } catch (e) { setErr(String(e)); setDevice(null); }
      };
      polling.current = window.setTimeout(poll, code.interval * 1000);
    } catch (e) { setErr(String(e)); }
  }

  if (!status) return null;
  if (status.login) {
    return (
      <div className={`flex items-center gap-2 text-[12.5px] ${compact ? "px-1" : ""}`}>
        <GithubMark />
        <span className="flex-1">Connected as <span className="font-semibold">@{status.login}</span></span>
        <button className="btn btn-ghost !py-1 text-[12px]" onClick={() => run(api.githubDisconnect)}>Disconnect</button>
      </div>
    );
  }

  return (
    <section className={`${compact ? "" : "tile"} space-y-2.5`}>
      {!compact && (
        <div>
          <p className="flex items-center gap-2 text-[13px] font-semibold"><GithubMark />Connect GitHub</p>
          <p className="mt-1 text-[11.5px] leading-relaxed text-muted">See all your repos here, run tasks on them in the cloud, and clone one in a click. The token stays on this computer.</p>
        </div>
      )}
      {device ? (
        <div className="space-y-1.5 text-center">
          <p className="text-[11.5px] text-muted">Enter this code on the GitHub page that just opened</p>
          <p className="tabular select-text font-mono text-[22px] font-semibold tracking-[0.2em]">{device.user_code}</p>
          <p className="text-[11px] text-mute">Waiting for GitHub…</p>
        </div>
      ) : (
        <div className="flex flex-wrap gap-1.5">
          {status.cli && <button disabled={busy} className="btn btn-primary grow" onClick={() => run(() => api.githubConnect())}><GithubMark />Use my GitHub CLI login</button>}
          {status.device_flow && <button disabled={busy} className={`btn grow ${status.cli ? "" : "btn-primary"}`} onClick={signIn}><GithubMark />Sign in with GitHub</button>}
          {!status.cli && !status.device_flow && !showToken && <button className="btn btn-primary grow" onClick={() => setShowToken(true)}><GithubMark />Connect with a token</button>}
        </div>
      )}
      {showToken ? (
        <div className="space-y-1.5">
          <div className="flex gap-1.5">
            <input className="field min-w-0 flex-1 font-mono" placeholder="ghp_… or github_pat_…" value={token} onFocus={() => api.focus()} onChange={(e) => setToken(e.target.value)} />
            <button disabled={busy || !token.trim()} className="btn" onClick={() => run(() => api.githubConnect(token))}>Connect</button>
          </div>
          <button className="text-[11px] text-muted hover:text-fg" onClick={() => api.openUrl(TOKEN_URL)}>Create a token on GitHub (needs the "repo" scope) →</button>
        </div>
      ) : (status.cli || status.device_flow) && !device ? (
        <button className="text-[11px] text-muted hover:text-fg" onClick={() => setShowToken(true)}>Or paste a token</button>
      ) : null}
      {err && <p className="text-[11.5px] text-bad">{err}</p>}
    </section>
  );
}

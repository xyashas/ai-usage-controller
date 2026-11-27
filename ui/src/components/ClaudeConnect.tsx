import { useState } from "react";
import type { ToolStatus } from "../types";
import { api } from "../api";

// claude.ai, the Claude desktop app and Claude Code share one set of plan limits.
// Only claude.ai can report them for chat users, so we ask them to sign in once.
export function ClaudeConnect({ tool, onChanged, compact = false, inline = false }: { tool?: ToolStatus; onChanged?: () => void; compact?: boolean; inline?: boolean }) {
  const [waiting, setWaiting] = useState(false);
  const [showKey, setShowKey] = useState(false);
  const [key, setKey] = useState("");
  const [err, setErr] = useState<string | null>(null);
  const expired = !!tool?.reconnect;

  async function connect() {
    setErr(null);
    try {
      await api.connectClaude();
      setWaiting(true);
    } catch (e) {
      setErr(String(e));
    }
  }

  async function useKey() {
    setErr(null);
    try {
      await api.focus();
      await api.claudeSessionKey(key);
      setKey("");
      setShowKey(false);
      setWaiting(true);
      onChanged?.();
    } catch (e) {
      setErr(String(e));
    }
  }

  const sub = inline ? "opacity-60" : "text-muted";
  return (
    <div className={inline ? "space-y-2" : `tile space-y-2 ${compact ? "!p-3" : ""}`}>
      {!compact && !inline && (
        <div className="space-y-1">
          <p className="text-[13px] font-medium">{expired ? "Reconnect your Claude account" : "Connect for live Claude usage"}</p>
          <p className="text-[11.5px] leading-relaxed text-muted">
            {expired
              ? "Your claude.ai session has expired."
              : "Sign in once to see how much of your 5-hour and weekly limits you’ve used across Claude."}
          </p>
        </div>
      )}
      {waiting ? (
        <p className={`text-[12px] ${inline ? "opacity-70" : "text-muted"}`}>Finish signing in in the Claude window. This updates within a minute.</p>
      ) : (
        <button onClick={connect} className={`btn w-full ${inline ? "btn-ink" : "btn-primary"}`}>
          {expired ? "Reconnect" : inline ? "Connect" : "Connect Claude account"}
        </button>
      )}
      {showKey ? (
        <div className="space-y-2">
          <p className={`text-[11px] leading-relaxed ${sub}`}>
            In your browser on claude.ai, open developer tools → Application → Cookies → claude.ai, and copy the value of <span className="font-mono">sessionKey</span>.
          </p>
          <div className="flex gap-1.5">
            <input
              value={key}
              onFocus={() => api.focus()}
              onChange={(e) => setKey(e.target.value)}
              placeholder="sk-ant-sid…"
              className={`min-w-0 flex-1 rounded-full px-3 py-1.5 font-mono text-[11.5px] outline-none ${inline ? "bg-white/60 text-black" : "field"}`}
            />
            <button disabled={!key.trim()} onClick={useKey} className={`btn !py-1.5 ${inline ? "btn-ink" : ""}`}>
              Use
            </button>
          </div>
        </div>
      ) : (
        <button onClick={() => setShowKey(true)} className={`block text-left text-[10.5px] ${sub} hover:opacity-100`}>
          Sign-in window not working? Paste a session key instead
        </button>
      )}
      {err && <p className={`text-[11.5px] ${inline ? "text-[#a3261b]" : "text-bad"}`}>{err}</p>}
    </div>
  );
}

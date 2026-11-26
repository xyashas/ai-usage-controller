import { useState } from "react";
import type { ToolStatus } from "../types";
import { api } from "../api";

// claude.ai, the Claude desktop app and Claude Code share one set of plan limits.
// Only claude.ai can report them for chat users, so we ask them to sign in once.
export function ClaudeConnect({ tool, onChanged, compact = false }: { tool?: ToolStatus; onChanged?: () => void; compact?: boolean }) {
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

  return (
    <div className={`space-y-2.5 rounded-2xl bg-white/[0.04] ring-1 ring-white/[0.07] ${compact ? "p-3" : "p-3.5"}`}>
      {!compact && (
        <div className="space-y-1">
          <p className="text-[13px] font-medium">{expired ? "Reconnect your Claude account" : "See your real Claude limits"}</p>
          <p className="text-[11.5px] leading-relaxed text-white/50">
            {expired
              ? "Your claude.ai session has expired."
              : "Sign in to claude.ai once. The widget then shows the same 5-hour and weekly numbers as claude.ai, including chats in the browser and the desktop app."}
          </p>
        </div>
      )}
      {waiting ? (
        <p className="text-[12px] text-white/70">Finish signing in in the Claude window. This panel updates within a minute.</p>
      ) : (
        <button onClick={connect} className="w-full rounded-full bg-white py-2 text-[12.5px] font-semibold text-black hover:bg-white/90">
          {expired ? "Reconnect" : "Connect Claude account"}
        </button>
      )}
      {showKey ? (
        <div className="space-y-1.5">
          <p className="text-[11px] leading-relaxed text-white/45">
            In your browser on claude.ai, open developer tools → Application → Cookies → claude.ai, and copy the value of <span className="font-mono text-white/70">sessionKey</span>.
          </p>
          <div className="flex gap-1.5">
            <input
              value={key}
              onFocus={() => api.focus()}
              onChange={(e) => setKey(e.target.value)}
              placeholder="sk-ant-sid…"
              className="min-w-0 flex-1 rounded-full bg-black/60 px-3 py-1.5 font-mono text-[11.5px] outline-none ring-1 ring-white/10 focus:ring-accent"
            />
            <button disabled={!key.trim()} onClick={useKey} className="rounded-full bg-white/10 px-3 text-[12px] font-medium disabled:opacity-40">
              Use
            </button>
          </div>
        </div>
      ) : (
        <button onClick={() => setShowKey(true)} className="block text-left text-[11px] text-white/40 hover:text-white/70">
          Sign-in window not working? Paste a session key instead
        </button>
      )}
      {err && <p className="text-[11.5px] text-bad">{err}</p>}
    </div>
  );
}

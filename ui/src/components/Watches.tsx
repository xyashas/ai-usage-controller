import { useCallback, useEffect, useState } from "react";
import type { Watch } from "../types";
import { api, onWatchesChanged } from "../api";
import { CloseIcon } from "./icons";

/** "Watching" list: add and remove natural-language watches/reminders. */
export function Watches() {
  const [items, setItems] = useState<Watch[]>([]);
  const [input, setInput] = useState("");
  const [err, setErr] = useState("");
  const [busy, setBusy] = useState(false);

  const load = useCallback(() => {
    api.listWatches().then(setItems, () => {});
  }, []);

  useEffect(() => {
    load();
    const unsub = onWatchesChanged(load);
    return () => {
      unsub.then((u) => u());
    };
  }, [load]);

  async function add() {
    const t = input.trim();
    if (!t || busy) return;
    setBusy(true);
    setErr("");
    try {
      await api.addWatch(t);
      setInput("");
      load();
    } catch (e) {
      setErr(String(e));
    } finally {
      setBusy(false);
    }
  }

  async function remove(id: string) {
    setErr("");
    try {
      await api.removeWatch(id);
      load();
    } catch (e) {
      setErr(String(e));
    }
  }

  return (
    <section className="space-y-1.5 pt-1" aria-label="Watching">
      <div className="flex items-center justify-between px-1">
        <p className="eyebrow">Watching</p>
        {items.length > 0 && <span className="tabular text-[11px] text-mute">{items.length}</span>}
      </div>
      <div className="tile space-y-2">
        <div className="flex items-center gap-1.5">
          <input
            value={input}
            onFocus={() => api.focus()}
            onChange={(e) => {
              setInput(e.target.value);
              if (err) setErr("");
            }}
            onKeyDown={(e) => {
              if (e.key === "Enter") {
                e.preventDefault();
                add();
              }
            }}
            placeholder='Tell me when Claude resets, or "in 20 minutes"…'
            className="field !py-1.5 text-[12px]"
          />
          <button
            type="button"
            disabled={!input.trim() || busy}
            onClick={add}
            className="btn shrink-0 !px-3 !py-1.5 text-[12px]"
          >
            Watch
          </button>
        </div>
        {err && <p className="text-[11px] text-bad">{err}</p>}
        {items.length > 0 && (
          <div className="space-y-1">
            {items.map((w) => (
              <div key={w.id} className="group flex items-center justify-between gap-2 rounded-xl px-2 py-1.5 hover:bg-white/[0.04]">
                <span className="min-w-0 truncate text-[12px] text-fg/90">{w.label}</span>
                <button
                  type="button"
                  aria-label={`Remove watch ${w.label}`}
                  onClick={() => remove(w.id)}
                  className="icon-btn -mr-1 shrink-0 opacity-60 hover:opacity-100"
                >
                  <CloseIcon />
                </button>
              </div>
            ))}
          </div>
        )}
      </div>
    </section>
  );
}

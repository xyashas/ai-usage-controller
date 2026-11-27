import { useCallback, useEffect, useState } from "react";
import type { View } from "../App";
import type { InboxItem } from "../types";
import { api, onInboxChanged } from "../api";
import { ago } from "../format";
import { TopBar } from "./TopBar";
import { CloseIcon } from "./icons";

const mac = navigator.userAgent.includes("Mac");

function remindLabel(iso: string): string {
  const d = new Date(iso);
  const sameDay = d.toDateString() === new Date().toDateString();
  const time = d.toLocaleTimeString([], { hour: "numeric", minute: "2-digit" });
  return sameDay ? `Reminder ${time}` : `Reminder ${d.toLocaleDateString([], { weekday: "short" })} ${time}`;
}

function InboxRow({
  item,
  onDone,
  onDelete,
  onAsk,
  onMakeTask,
  onRemind,
}: {
  item: InboxItem;
  onDone: (done: boolean) => void;
  onDelete: () => void;
  onAsk: () => void;
  onMakeTask: () => void;
  onRemind: (when: string) => Promise<void>;
}) {
  const [copied, setCopied] = useState(false);
  const [reminding, setReminding] = useState(false);
  const [busy, setBusy] = useState(false);

  async function copy() {
    try {
      await navigator.clipboard.writeText(item.text);
      setCopied(true);
      setTimeout(() => setCopied(false), 1400);
    } catch {
      // Clipboard may be unavailable in non-secure browser previews.
    }
  }

  async function pickRemind(when: string) {
    setBusy(true);
    try {
      await onRemind(when);
      setReminding(false);
    } finally {
      setBusy(false);
    }
  }

  const meta = [item.app || item.source, ago(item.at)].filter(Boolean).join(" · ");

  return (
    <div className="group rounded-2xl px-3 py-2.5 transition-colors hover:bg-white/[0.04]">
      <div className="flex items-start gap-2.5">
        <button
          type="button"
          onClick={() => onDone(!item.done)}
          aria-label={item.done ? "Mark not done" : "Mark done"}
          title={item.done ? "Mark not done" : "Done"}
          className={`mt-0.5 flex h-4 w-4 shrink-0 items-center justify-center rounded-full border text-[10px] transition-colors ${
            item.done
              ? "border-[#4ade80]/60 bg-[#4ade80]/20 text-ok"
              : "border-white/25 text-transparent hover:border-white/50"
          }`}
        >
          ✓
        </button>
        <div className="min-w-0 flex-1 space-y-1">
          <p className={`select-text whitespace-pre-wrap text-[12.5px] leading-relaxed ${item.done ? "text-muted line-through" : "text-fg/95"}`}>
            {item.text}
          </p>
          <div className="flex flex-wrap items-center gap-x-2 gap-y-0.5 text-[11px] text-muted">
            <span>{meta}</span>
            {item.remind_at && !item.done && (
              <span className="text-accent">· {remindLabel(item.remind_at)}</span>
            )}
          </div>
          {!item.done && (
            <div className="flex flex-wrap items-center gap-1 pt-0.5 text-[11px]">
              <button type="button" onClick={copy} className="rounded-full bg-white/[0.06] px-2 py-0.5 text-muted transition-colors hover:bg-white/[0.12] hover:text-fg">
                {copied ? "Copied" : "Copy"}
              </button>
              <button type="button" onClick={onAsk} className="rounded-full bg-white/[0.06] px-2 py-0.5 text-muted transition-colors hover:bg-white/[0.12] hover:text-fg">
                Ask
              </button>
              <button type="button" onClick={onMakeTask} className="rounded-full bg-white/[0.06] px-2 py-0.5 text-muted transition-colors hover:bg-white/[0.12] hover:text-fg">
                Make a task
              </button>
              <button
                type="button"
                onClick={() => setReminding(!reminding)}
                className={`rounded-full px-2 py-0.5 transition-colors ${
                  reminding ? "bg-white/[0.16] text-fg" : "bg-white/[0.06] text-muted hover:bg-white/[0.12] hover:text-fg"
                }`}
              >
                Remind me
              </button>
            </div>
          )}
          {reminding && !item.done && (
            <div className="fade-in flex items-center gap-1.5 pt-1 text-[11px]">
              <button type="button" disabled={busy} onClick={() => pickRemind("later today")} className="btn !px-2.5 !py-1 text-[11px]">
                Later today
              </button>
              <button type="button" disabled={busy} onClick={() => pickRemind("tomorrow 9am")} className="btn !px-2.5 !py-1 text-[11px]">
                Tomorrow 9am
              </button>
            </div>
          )}
        </div>
        <button
          type="button"
          onClick={onDelete}
          aria-label="Delete item"
          title="Delete"
          className="icon-btn -mr-1 shrink-0 opacity-0 transition-opacity group-hover:opacity-100 focus:opacity-100"
        >
          <CloseIcon />
        </button>
      </div>
    </div>
  );
}

/** Local "Remember this" inbox view. */
export function Inbox({ onView, onTask }: { onView: (v: View) => void; onTask: (text: string) => void }) {
  const [items, setItems] = useState<InboxItem[]>([]);
  const [showDone, setShowDone] = useState(false);
  const [err, setErr] = useState("");

  const load = useCallback(() => {
    api.inboxList().then(setItems, (e) => setErr(String(e)));
  }, []);

  useEffect(() => {
    load();
    const unsub = onInboxChanged(load);
    return () => {
      unsub.then((u) => u());
    };
  }, [load]);

  const open = items.filter((i) => !i.done);
  const done = items.filter((i) => i.done);

  async function makeTask(item: InboxItem) {
    setErr("");
    try {
      const text = await api.inboxToTask(item.id);
      onTask(text || item.text);
    } catch (e) {
      setErr(String(e));
    }
  }

  return (
    <div className="flex flex-col gap-2.5 p-3">
      <TopBar tab="inbox" onView={onView} />

      <div className="flex items-center justify-between px-1 pt-0.5">
        <p className="eyebrow">Inbox</p>
        {open.length > 0 && <span className="tabular text-[11px] text-mute">{open.length} saved</span>}
      </div>

      {err && <p className="px-1 text-[11.5px] text-bad">{err}</p>}

      {open.length > 0 ? (
        <section className="tile fade-in !p-1">
          {open.map((item) => (
            <InboxRow
              key={item.id}
              item={item}
              onDone={(d) => api.inboxDone(item.id, d).then(load, (e) => setErr(String(e)))}
              onDelete={() => api.inboxRemove(item.id).then(load, (e) => setErr(String(e)))}
              onAsk={() => window.dispatchEvent(new CustomEvent("aiuc-ask", { detail: { text: item.text, action: "ask" } }))}
              onMakeTask={() => makeTask(item)}
              onRemind={async (when) => {
                setErr("");
                try {
                  await api.inboxRemind(item.id, when);
                  load();
                } catch (e) {
                  setErr(String(e));
                }
              }}
            />
          ))}
        </section>
      ) : done.length === 0 ? (
        <section className="tile fade-in space-y-1.5 py-5 text-center">
          <p className="text-[13px] font-medium text-fg/90">Nothing saved yet</p>
          <p className="text-[12px] leading-relaxed text-muted">
            Hold <span className="font-medium text-fg">{mac ? "⌥⇧Space" : "Alt+Shift+Space"}</span> and say <span className="text-fg">&ldquo;save this&rdquo;</span>.
          </p>
        </section>
      ) : (
        <p className="px-1 text-[12px] text-muted">All caught up.</p>
      )}

      {done.length > 0 && (
        <section className="space-y-1.5">
          <div className="flex items-center justify-between px-1">
            <button
              type="button"
              onClick={() => setShowDone(!showDone)}
              className="flex items-center gap-1.5 text-[11.5px] font-medium text-muted hover:text-fg"
            >
              <span>{showDone ? "▾" : "▸"}</span>
              <span>Done ({done.length})</span>
            </button>
            {showDone && (
              <button
                type="button"
                onClick={() => api.inboxClearDone().then(load, (e) => setErr(String(e)))}
                className="text-[11px] text-muted hover:text-fg"
              >
                Clear
              </button>
            )}
          </div>
          {showDone && (
            <div className="tile fade-in !p-1">
              {done.map((item) => (
                <InboxRow
                  key={item.id}
                  item={item}
                  onDone={(d) => api.inboxDone(item.id, d).then(load, (e) => setErr(String(e)))}
                  onDelete={() => api.inboxRemove(item.id).then(load, (e) => setErr(String(e)))}
                  onAsk={() => window.dispatchEvent(new CustomEvent("aiuc-ask", { detail: { text: item.text, action: "ask" } }))}
                  onMakeTask={() => makeTask(item)}
                  onRemind={async () => {}}
                />
              ))}
            </div>
          )}
        </section>
      )}
    </div>
  );
}

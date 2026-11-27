import { useEffect, useRef, useState } from "react";
import type { View } from "../App";
import type { AskAction, AskContext, AskDone, AskMode, AskTurn, Tool } from "../types";
import { api, onAsk } from "../api";
import { tint, toolName } from "../theme";
import { ToolMark } from "./ToolMark";
import { Markdown, plain } from "./Markdown";
import { BackIcon, CloseIcon, MicIcon } from "./icons";
import { PolishButton, PolishDiffCard, usePromptPolish } from "./PromptPolish";

const mac = navigator.userAgent.includes("Mac");
const pref = (k: string, d: string) => { try { return localStorage.getItem(`ask.${k}`) ?? d; } catch { return d; } };
const setPref = (k: string, v: string) => { try { localStorage.setItem(`ask.${k}`, v); } catch { /* private mode */ } };

const ERRORISH = /error|exception|traceback|panicked|failed|undefined|not found|cannot|\bat \S+:\d+/i;
const CODEISH = /[{};]\s*$|=>|\bdef |\bfn |function |const |import |<\/?\w+>/m;
const WATCH = /^\s*(remind me|tell me when|let me know when|ping me when|notify me when|alert me when)\b/i;

type Answer = { text: string; done?: AskDone };
type Item = {
  id: string; question: string; action: AskAction; label: string;
  answer: Answer; council?: Partial<Record<Tool | "combined", Answer>>; note?: string;
};

const actionLabel: Record<AskAction, string> = {
  ask: "", explain: "Explain", fix: "Fix", find_bug: "Find the bug", rewrite: "Rewrite", shorter: "Shorter",
  professional: "More professional", translate: "Translate", reply: "Reply to this", summarize: "Summarize", point: "Where do I click?",
};

function quickActions(ctx: AskContext | null): AskAction[] {
  const text = ctx?.selection ?? ctx?.clipboard ?? "";
  if (!text && ctx?.screenshot) return ["explain", "point", "summarize"];
  if (!text) return [];
  if (ERRORISH.test(text) && text.length < 6000) return ["fix", "explain", "find_bug"];
  if (CODEISH.test(text)) return ["explain", "find_bug", "fix"];
  return ["rewrite", "shorter", "professional", "reply", "translate", "summarize"];
}

/** A shell command worth offering "Run" for. */
function command(answer: string): string | null {
  for (const m of answer.matchAll(/```(\w*)\n([\s\S]*?)```/g)) {
    const body = m[2].trim().replace(/^\$\s+/gm, "");
    const shell = /^(bash|sh|shell|zsh|console|powershell|ps1|pwsh|cmd|bat)$/i.test(m[1]);
    const looks = !m[1] && /^(npm|pnpm|yarn|npx|git|cargo|pip|python|go|make|docker|kubectl|brew|winget|cd|ls|dir)\b/.test(body);
    if ((shell || looks) && body && body.split("\n").length <= 6) return body;
  }
  return null;
}

const REWRITES: AskAction[] = ["rewrite", "shorter", "professional", "translate", "reply"];

function Chip({ children, onRemove, title }: { children: React.ReactNode; onRemove?: () => void; title?: string }) {
  return (
    <span title={title} className="fade-in flex max-w-[220px] items-center gap-1.5 rounded-full bg-white/[0.07] py-1 pl-2.5 pr-1 text-[11px] text-muted">
      <span className="truncate">{children}</span>
      {onRemove && <button aria-label="Remove" className="rounded-full p-0.5 hover:bg-white/10 hover:text-fg" onClick={onRemove}><CloseIcon /></button>}
    </span>
  );
}

function AnswerActions({ item, ctx, onTask, speak }: {
  item: Item; ctx: AskContext | null; onTask: (text: string) => void; speak: (t: string) => void;
}) {
  const [note, setNote] = useState("");
  const done = (item.council?.combined ?? item.answer).done;
  const text = (item.council?.combined ?? item.answer).text.trim();
  if (!done || done.error || !text) return null;
  const run = command(text);
  const fixable = item.action === "fix" || ERRORISH.test(ctx?.selection ?? ctx?.clipboard ?? "");
  // Rewrites go back where they came from; other answers are copied.
  const rewrite = REWRITES.includes(item.action);
  const where = ctx?.app && ctx.app.length <= 18 ? ctx.app : "the app";
  const pasteLabel = !rewrite ? null : item.action === "reply" ? `Paste reply into ${where}` : ctx?.selection ? "Replace" : `Paste into ${where}`;
  const act = (f: () => Promise<unknown>, ok: string) => f().then((m) => setNote(typeof m === "string" && m ? m : ok), (e) => setNote(String(e)));
  const tool = done.tool ?? "claude_code";
  return (
    <div className="space-y-1.5">
      <div className="flex flex-wrap gap-1.5">
        {pasteLabel && <button className="btn btn-primary !px-3 !py-1.5 text-[11.5px]" onClick={() => act(() => api.askPasteBack(text), "Pasted.")}>{pasteLabel}</button>}
        <button className={`btn !px-3 !py-1.5 text-[11.5px] ${pasteLabel ? "" : "btn-primary"}`} onClick={() => act(() => api.askCopy(text), "Copied.")}>Copy</button>
        {run && <button className="btn !px-3 !py-1.5 text-[11.5px]" title={run} onClick={() => act(() => api.askRun(run), "")}>Run in terminal</button>}
        {fixable && <button className="btn !px-3 !py-1.5 text-[11.5px]" onClick={() => onTask(`Fix this:\n\n${ctx?.selection ?? ctx?.clipboard ?? item.question}\n\nSuggested fix:\n${text}`)}>Fix it with an agent</button>}
        {!fixable && <button className="btn btn-ghost !px-3 !py-1.5 text-[11.5px]" onClick={() => onTask(item.question || text)}>Make it a task</button>}
        <button className="btn btn-ghost !px-3 !py-1.5 text-[11.5px]" onClick={() => act(() => api.askOpenIn(tool, `${item.question}\n\n${text}`), "")}><ToolMark tool={tool} />Open in {toolName[tool]}</button>
        <button className="btn btn-ghost !px-2.5 !py-1.5 text-[11.5px]" title="Read it aloud" onClick={() => speak(text)}>Listen</button>
        {done.points.length > 0 && <button className="btn btn-ghost !px-2.5 !py-1.5 text-[11.5px]" onClick={() => api.askHidePoints()}>Hide arrows</button>}
      </div>
      {note && <p className="text-[11px] text-muted">{note}</p>}
    </div>
  );
}

function AnswerBody({ a, tool }: { a: Answer; tool?: Tool | null }) {
  if (a.done?.error) return <p className="text-[12px] text-bad">{a.done.error}</p>;
  if (a.done?.opened_web) return <p className="text-[12px] text-muted">{a.done.reason}. No AI command-line tool is installed here, so it opened on the web.</p>;
  const shown = a.done ? a.done.text || a.text : a.text;
  if (!shown.trim()) return <p className="flex items-center gap-2 text-[12px] text-muted"><span className="live-dot h-1.5 w-1.5 rounded-full" style={{ background: tool ? tint[tool].bar : "#fff" }} />Thinking…</p>;
  return <Markdown text={shown.replace(/<point[^>]*\/>/g, "")} />;
}

type Saved = { q: string; a: string; at: number };
const loadHistory = (): Saved[] => { try { return JSON.parse(localStorage.getItem("ask.saved") ?? "[]"); } catch { return []; } };
let lastQuestion = "";
function saveHistory(answer: string) {
  try { localStorage.setItem("ask.saved", JSON.stringify([{ q: lastQuestion, a: answer, at: Date.now() }, ...loadHistory()].slice(0, 20))); } catch { /* full or private */ }
}

export function AskView({ onView, onTask, init }: { onView: (v: View) => void; onTask: (text: string) => void; init: { text: string; action: string } | null }) {
  const [ctx, setCtx] = useState<AskContext | null>(null);
  const [text, setText] = useState("");
  const [items, setItems] = useState<Item[]>([]);
  const [mode, setMode] = useState<AskMode>(pref("mode", "quick") as AskMode);
  const [pinned, setPinned] = useState<Tool | null>((pref("tool", "") || null) as Tool | null);
  const [council, setCouncil] = useState(false);
  const [voice, setVoice] = useState(pref("voice", "0") === "1");
  const [busy, setBusy] = useState<string | null>(null);
  const box = useRef<HTMLTextAreaElement>(null);
  const end = useRef<HTMLDivElement>(null);
  const polish = usePromptPolish(text, null);
  const voiceRef = useRef(voice);
  voiceRef.current = voice;

  // Just asked out loud? Carry that conversation on here.
  useEffect(() => {
    if (init) return;
    api.lastVoiceTurn().then((v) => {
      if (!v || !v.answer) return;
      setItems((all) => all.length ? all : [{ id: "voice", question: v.question, action: "ask", label: v.question, answer: { text: v.answer, done: { id: "voice", tool: v.tool, reason: v.reason, error: null, points: [], text: v.answer, opened_web: false } } }]);
    }, () => {});
  }, []);

  const loadCtx = (p: Tool | null) => api.askContext(p).then(setCtx, () => setCtx(null));
  useEffect(() => { loadCtx(pinned); }, [pinned]);
  useEffect(() => { api.focus().finally(() => box.current?.focus({ preventScroll: true })); }, []);

  const speak = (t: string) => {
    if (!("speechSynthesis" in window)) return;
    speechSynthesis.cancel();
    speechSynthesis.speak(new SpeechSynthesisUtterance(plain(t).slice(0, 1500)));
  };

  // Stream answers into their item: "<id>", or "<id>:<tool>" / "<id>:combined" for Council.
  useEffect(() => {
    const sub = onAsk(
      (id, piece) => setItems((all) => all.map((it) => {
        const [base, part] = id.split(":");
        if (it.id !== base) return it;
        if (!part) return { ...it, answer: { ...it.answer, text: it.answer.text + piece } };
        const cur = it.council?.[part as Tool] ?? { text: "" };
        return { ...it, council: { ...it.council, [part]: { ...cur, text: cur.text + piece } } };
      })),
      (d) => {
        const [base, part] = d.id.split(":");
        setItems((all) => all.map((it) => {
          if (it.id !== base) return it;
          if (!part) return { ...it, answer: { text: it.answer.text || d.text || "", done: d } };
          const cur = it.council?.[part as Tool] ?? { text: "" };
          return { ...it, council: { ...it.council, [part]: { ...cur, done: d } } };
        }));
        if (!part || part === "combined") {
          setBusy((b) => (b === base ? null : b));
          if (!d.error && d.text && pref("history", "0") === "1") saveHistory(d.text);
          if (voiceRef.current && !d.error && d.text) speak(d.text);
        }
      },
    );
    return () => { sub.then((u) => u()); };
  }, []);

  useEffect(() => { end.current?.scrollIntoView({ block: "end", behavior: "smooth" }); }, [items]);

  async function ask(question: string, action: AskAction = "ask") {
    if (busy) return;
    const q = question.trim();
    if (!q && action === "ask" && !ctx?.selection && !ctx?.clipboard && !ctx?.screenshot) return;
    const id = `a${Date.now()}`;
    setText("");
    lastQuestion = q || actionLabel[action];
    // "Tell me when…" goes to the watchers, not to an AI.
    if (action === "ask" && WATCH.test(q)) {
      const item: Item = { id, question: q, action, label: q, answer: { text: "" } };
      try {
        const w = await api.addWatch(q);
        item.answer = { text: `Watching: **${w.label}**. You'll get a notification.`, done: { id, tool: null, reason: "", error: null, points: [], text: "", opened_web: false } };
      } catch (e) {
        item.answer = { text: "", done: { id, tool: null, reason: "", error: String(e).includes("add_watch") ? "Reminders arrive in the next update." : String(e), points: [], text: "", opened_web: false } };
      }
      setItems((all) => [...all, item]);
      return;
    }
    const history: AskTurn[] = items.filter((i) => i.answer.done && !i.answer.done.error).map((i) => ({ question: i.question || actionLabel[i.action], answer: (i.council?.combined ?? i.answer).text }));
    setItems((all) => [...all, { id, question: q, action, label: q || actionLabel[action], answer: { text: "" }, council: council ? {} : undefined }]);
    setBusy(id);
    try { await api.askStart({ id, question: q, action, mode, tool: pinned, history, council }); }
    catch (e) {
      setBusy(null);
      setItems((all) => all.map((it) => it.id === id ? { ...it, answer: { text: "", done: { id, tool: null, reason: "", error: String(e), points: [], text: "", opened_web: false } } } : it));
    }
  }

  // Chips and other parts of the app can start an ask (see SuggestionChip).
  const started = useRef(false);
  useEffect(() => {
    if (!init || started.current || !ctx) return;
    started.current = true;
    if (init.action === "ask_fix") ask(`Fix this error:\n\n${init.text}`, "fix");
    else ask(init.text);
  }, [init, ctx]);

  function stop() { if (busy) { api.askCancel(busy); setBusy(null); } }

  function cycleTool() {
    const order: (Tool | null)[] = [null, ...(ctx?.installed ?? [])];
    const next = order[(order.indexOf(pinned) + 1) % order.length];
    setPinned(next);
    setPref("tool", next ?? "");
  }

  const forget = (part: "selection" | "clipboard" | "screenshot") => api.askForget(part).then(() => loadCtx(pinned));
  const actions = quickActions(ctx);
  const pick = ctx?.pick;
  const shownTool = pinned ?? pick?.tool ?? null;

  return (
    <div className="flex flex-col gap-2.5 p-3">
      <header className="flex items-center gap-2 px-1">
        <button className="icon-btn" aria-label="Back" onClick={() => { stop(); onView("panel"); }}><BackIcon /></button>
        <h2 className="text-[15px] font-semibold tracking-tight">Ask</h2>
        <div className="ml-auto flex items-center gap-1.5">
          <button
            onClick={cycleTool}
            title={pinned ? "You picked this tool. Click to change." : `Auto: ${pick?.reason ?? "the tool with most room"}. Click to pick one.`}
            className="flex items-center gap-1.5 rounded-full px-2.5 py-1 text-[11.5px] font-semibold"
            style={shownTool ? { background: tint[shownTool].bg, color: tint[shownTool].ink } : { background: "rgba(255,255,255,0.08)" }}
          >
            {shownTool && <ToolMark tool={shownTool} />}
            {pinned ? toolName[pinned] : shownTool ? `Auto · ${toolName[shownTool]}` : "No AI installed"}
          </button>
          <div className="seg" role="group" aria-label="Mode">
            <button aria-pressed={mode === "quick"} title="Fast model, short answers" onClick={() => { setMode("quick"); setPref("mode", "quick"); }}>Quick</button>
            <button aria-pressed={mode === "deep"} title="The strongest model" onClick={() => { setMode("deep"); setPref("mode", "deep"); }}>Deep</button>
          </div>
        </div>
      </header>

      {items.length > 0 && (
        <div className="space-y-3">
          {items.map((it) => {
            const d = it.answer.done;
            return (
              <section key={it.id} className="fade-in space-y-2">
                {it.label && <p className="ml-auto w-fit max-w-[85%] whitespace-pre-wrap rounded-2xl rounded-br-md bg-white/[0.09] px-3 py-2 text-[12.5px]">{it.label.length > 400 ? `${it.label.slice(0, 400)}…` : it.label}</p>}
                {it.council ? (
                  <div className="space-y-2">
                    <div className="grid grid-cols-3 gap-1.5">
                      {(["claude_code", "codex", "gemini"] as Tool[]).filter((t) => ctx?.installed.includes(t)).map((t) => {
                        const a = it.council?.[t];
                        return (
                          <details key={t} className="tile !p-2.5 text-[11.5px]">
                            <summary className="flex cursor-pointer list-none items-center gap-1.5 font-semibold">
                              <ToolMark tool={t} />{toolName[t]}
                              <span className="ml-auto text-[10.5px] font-normal text-muted">{a?.done ? (a.done.error ? "failed" : "done") : "…"}</span>
                            </summary>
                            <div className="mt-2 max-h-48 overflow-auto">{a ? <AnswerBody a={a} tool={t} /> : null}</div>
                          </details>
                        );
                      })}
                    </div>
                    <div className="card space-y-2 !p-3.5">
                      <p className="eyebrow">Council</p>
                      {it.council.combined ? <AnswerBody a={it.council.combined} /> : <p className="text-[12px] text-muted">Waiting for the answers to compare…</p>}
                    </div>
                  </div>
                ) : (
                  <div className="card space-y-2 !p-3.5">
                    <AnswerBody a={it.answer} tool={shownTool} />
                  </div>
                )}
                {(d || it.council?.combined?.done) && !(d?.error) && (
                  <p className="px-1 text-[10.5px] text-mute">
                    {(() => { const dd = it.council?.combined?.done ?? d; return dd?.tool ? (dd.reason.startsWith("combined") ? `Combined by ${toolName[dd.tool]}` : `${toolName[dd.tool]}${dd.reason ? ` · ${dd.reason}` : ""}`) : ""; })()}
                  </p>
                )}
                <AnswerActions item={it} ctx={ctx} onTask={onTask} speak={speak} />
              </section>
            );
          })}
          <div ref={end} />
        </div>
      )}

      {items.length === 0 && (ctx?.app || ctx?.selection || ctx?.clipboard || ctx?.screenshot || ctx?.repo) && (
        <div className="flex flex-wrap items-center gap-1.5 px-0.5">
          {ctx.screenshot && (
            <span className="fade-in relative">
              <img src={ctx.screenshot} alt="Screenshot of your window" className="h-9 rounded-lg ring-1 ring-white/10" />
              <button aria-label="Don't send the screenshot" className="absolute -right-1.5 -top-1.5 rounded-full bg-[#2a2a2e] p-0.5 text-muted hover:text-fg" onClick={() => forget("screenshot")}><CloseIcon /></button>
            </span>
          )}
          {ctx.app && <Chip title={ctx.window ?? undefined}>{ctx.app}</Chip>}
          {ctx.selection && <Chip title={ctx.selection} onRemove={() => forget("selection")}>Selected: “{ctx.selection.slice(0, 40)}”</Chip>}
          {(ctx.clipboard || ctx.clipboard_image) && <Chip title={ctx.clipboard ?? "An image you copied"} onRemove={() => forget("clipboard")}>{ctx.clipboard ? `Copied: “${ctx.clipboard.slice(0, 32)}”` : "Copied image"}</Chip>}
          {ctx.repo && <Chip>Repo: {ctx.repo}</Chip>}
        </div>
      )}

      {items.length === 0 && actions.length > 0 && (
        <div className="flex flex-wrap gap-1.5 px-0.5">
          {actions.map((a) => (
            <button key={a} disabled={!!busy} className="btn !px-3 !py-1.5 text-[12px]" onClick={() => ask("", a)}>{actionLabel[a]}</button>
          ))}
        </div>
      )}

      {items.length === 0 && !ctx?.selection && !ctx?.screenshot && pref("history", "0") === "1" && loadHistory().length > 0 && (
        <div className="space-y-1 px-0.5">
          <p className="eyebrow px-1">Recent</p>
          {loadHistory().slice(0, 4).map((h) => (
            <button key={h.at} className="block w-full truncate rounded-xl px-2.5 py-1.5 text-left text-[12px] text-muted hover:bg-white/[0.05] hover:text-fg" onClick={() => setText(h.q)}>{h.q}</button>
          ))}
        </div>
      )}

      <section className="tile !p-0">
        <textarea
          ref={box}
          rows={items.length ? 1 : 2}
          value={text}
          onFocus={() => api.focus()}
          onChange={(e) => setText(e.target.value)}
          onKeyDown={(e) => { if (e.key === "Enter" && !e.shiftKey) { e.preventDefault(); ask(text); } }}
          placeholder={items.length ? "Ask a follow-up…" : ctx?.selection || ctx?.screenshot ? "Ask about this, or pick an action" : "Ask anything. Try: What are my agents doing?"}
          className="block w-full resize-none bg-transparent px-4 pb-1.5 pt-3 text-[13.5px] leading-relaxed outline-none placeholder:text-mute"
          style={{ outline: "none" }}
        />
        <div className="flex items-center gap-1.5 px-2.5 pb-2.5">
          <button className="icon-btn" title={mac ? "Dictate" : "Dictate (Win+H)"} aria-label="Dictate" onClick={async () => { await api.focus(); box.current?.focus(); api.startDictation().catch(() => {}); if (!voice) { setVoice(true); setPref("voice", "1"); } }}><MicIcon /></button>
          <button
            role="switch" aria-checked={voice}
            title="Read answers aloud"
            onClick={() => { setVoice(!voice); setPref("voice", voice ? "0" : "1"); if (voice) speechSynthesis?.cancel(); }}
            className={`rounded-full px-2.5 py-1 text-[11px] font-medium ${voice ? "bg-white/15 text-fg" : "text-muted hover:text-fg"}`}
          >
            {voice ? "Speaking on" : "Speak"}
          </button>
          <button
            role="switch" aria-checked={council}
            disabled={(ctx?.installed.length ?? 0) < 2}
            title={(ctx?.installed.length ?? 0) < 2 ? "Council needs two or more AI tools installed" : "Ask Claude, Codex and Gemini at once, then combine the answers"}
            onClick={() => setCouncil(!council)}
            className={`rounded-full px-2.5 py-1 text-[11px] font-medium disabled:opacity-40 ${council ? "bg-[#DDD7F3] text-[#1C1535]" : "text-muted hover:text-fg"}`}
          >
            Council
          </button>
          <span className="ml-auto" />
          {!busy && text.trim().split(/\s+/).length >= 3 && <PolishButton disabled={false} busy={polish.busy} onClick={polish.polish} />}
          {busy ? (
            <button className="btn ml-auto !py-1.5" onClick={stop}>Stop</button>
          ) : (
            <button disabled={!text.trim() && !(items.length === 0 && (ctx?.selection || ctx?.screenshot))} className="btn btn-primary !py-1.5" onClick={() => ask(text)}>
              Ask <span className="text-[10.5px] font-medium opacity-50">↵</span>
            </button>
          )}
        </div>
      </section>
      {polish.error && <p className="px-1 text-[11.5px] text-bad">{polish.error}</p>}
      {polish.draft && <PolishDiffCard original={polish.draft.original} polished={polish.draft.polished} onAccept={(next) => { setText(next); polish.clear(); box.current?.focus(); }} onKeep={polish.clear} />}
      <p className="px-1 text-[10.5px] text-mute">
        {mac ? "⌥⇧Space" : "Alt+Shift+Space"} from anywhere. Uses the AI tools you already have; {pref("history", "0") === "1" ? "the last 20 answers are kept on this computer" : "nothing is kept unless you say \u201cremember\u201d"}.
      </p>
    </div>
  );
}

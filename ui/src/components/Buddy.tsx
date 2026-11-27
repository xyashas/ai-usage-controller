import { useEffect, useRef, useState } from "react";
import { api, onAsk, onBuddy } from "../api";
import type { BuddyState } from "../types";

const mac = navigator.userAgent.includes("Mac");
const KEY = mac ? "⌥⇧Space" : "Alt+Shift+Space";

/** Only pick a genuinely natural/neural voice; skip robotic legacy SAPI5 desktop voices (David/Zira/Mark). */
function bestVoice(): SpeechSynthesisVoice | undefined {
  const all = speechSynthesis.getVoices();
  const lang = navigator.language.slice(0, 2);
  const mine = all.filter((v) => v.lang.startsWith(lang));
  const pool = mine.length ? mine : all.filter((v) => v.lang.startsWith("en"));
  const nonRobotic = pool.filter((v) => !/\b(desktop|david|zira|mark|hazel|george|heera|ravi)\b/i.test(v.name));
  return nonRobotic.find((v) => /natural|neural/i.test(v.name))
    ?? nonRobotic.find((v) => /online/i.test(v.name))
    ?? nonRobotic.find((v) => /aria|jenny|ava|andrew|emma|brian|guy|samantha|allison|daniel|karen|serena|enhanced|premium/i.test(v.name));
}

/** A soft two-note blip: up when listening starts, down when it stops. */
function chime(up: boolean) {
  try {
    const ctx = new AudioContext();
    const notes = up ? [660, 880] : [880, 620];
    notes.forEach((f, i) => {
      const o = ctx.createOscillator();
      const g = ctx.createGain();
      o.type = "sine";
      o.frequency.value = f;
      const t = ctx.currentTime + i * 0.07;
      g.gain.setValueAtTime(0, t);
      g.gain.linearRampToValueAtTime(0.06, t + 0.015);
      g.gain.exponentialRampToValueAtTime(0.0001, t + 0.16);
      o.connect(g).connect(ctx.destination);
      o.start(t);
      o.stop(t + 0.18);
    });
    setTimeout(() => ctx.close(), 500);
  } catch { /* no audio */ }
}

/** One rotating tip under an answer, so voice commands are easy to find. */
const TIPS = [
  "Hold again and say “paste that”",
  "Say “shorter” or “more casual” to redo it",
  "Say “send it to Codex” to hand it off",
  "Say “remember that I…” and I'll keep it in mind",
  "Say “save this” to put it in your inbox",
  `Tap ${KEY} to keep going in the panel`,
];
let tipAt = Math.floor(Math.random() * TIPS.length);
function Tip() {
  const [t] = useState(() => TIPS[tipAt++ % TIPS.length]);
  return <>{t}</>;
}

const clean = (t: string) => t.replace(/\*\*|`|#+ /g, "").replace(/<point[^>]*\/>/g, "").trim();

/** The see-through window over your screen: live captions while you talk,
 *  the answer in one card by your cursor (read aloud, words lighting up as
 *  they're spoken), and a little pointer that flies to what to click. */
export function Buddy() {
  const [b, setB] = useState<BuddyState | null>(null);
  const [stream, setStream] = useState("");
  const [spoken, setSpoken] = useState(-1); // characters read aloud so far; -1 = not reading
  const [from, setFrom] = useState<[number, number] | null>(null);
  const [leaving, setLeaving] = useState(false);
  const idRef = useRef("");
  const modeRef = useRef("");
  const fade = useRef<number | undefined>(undefined);

  useEffect(() => {
    document.documentElement.style.background = "transparent";
    document.body.style.background = "transparent";
    speechSynthesis?.getVoices(); // loads the list early

    const say = (text: string, then: () => void) => {
      if (!("speechSynthesis" in window)) return then();
      const v = bestVoice();
      // Skip auto-reading aloud when only legacy robotic desktop voices are installed.
      if (!v) return then();
      speechSynthesis.cancel();
      const u = new SpeechSynthesisUtterance(text.slice(0, 900));
      u.voice = v;
      u.rate = 1.0;
      setSpoken(0);
      u.onboundary = (e) => setSpoken(e.charIndex + (e.charLength || 0));
      u.onend = () => { setSpoken(text.length); then(); };
      speechSynthesis.speak(u);
    };

    const apply = (s: BuddyState) => {
      clearTimeout(fade.current);
      setLeaving(false);
      if (s.mode === "listening" && modeRef.current !== "listening") chime(true);
      if (modeRef.current === "listening" && s.mode !== "listening") chime(false);
      modeRef.current = s.mode;
      if (s.id !== idRef.current) { setStream(""); setSpoken(-1); }
      idRef.current = s.id;
      setB((prev) => {
        setFrom(prev?.spots[0] ? [prev.spots[0].fx, prev.spots[0].fy] : s.cursor);
        return s;
      });
      // Quick confirmations ("Pasted", "Saved") leave fast; real answers stay a while.
      const linger = s.mode === "error" ? 9000 : s.mode === "done" ? (s.text.length < 60 ? 2200 : 6000) : s.mode === "guide" ? 120000 : 0;
      if (s.speak) say(clean(s.speak), () => { if (s.mode === "answer" || s.mode === "done") fade.current = window.setTimeout(hide, 8000); });
      else if (linger) fade.current = window.setTimeout(hide, linger);
    };

    api.buddyState().then((s) => s && apply(s), () => {});
    const subs = [
      onBuddy((s) => (s.mode === "hide" ? hide(true) : apply(s))),
      onAsk((id, piece) => { if (id === idRef.current) setStream((t) => t + piece); }, () => {}),
    ];
    return () => { subs.forEach((p) => p.then((u) => u())); };
  }, []);

  function hide(fromNative = false) {
    clearTimeout(fade.current);
    setLeaving(true);
    speechSynthesis?.cancel();
    window.setTimeout(() => { setB(null); setLeaving(false); if (!fromNative) api.buddyHide(); }, 220);
  }

  if (!b) return null;
  const spot = b.spots[0];
  const [ax, ay] = spot ? [spot.fx, spot.fy] : b.cursor;
  const left = ax > 0.68 ? `calc(${ax * 100}% - 356px)` : `calc(${ax * 100}% + 28px)`;
  const top = ay > 0.72 ? `calc(${ay * 100}% - 170px)` : `calc(${ay * 100}% + 24px)`;
  const body = clean(b.mode === "answer" && !b.text ? stream : b.text);
  const listening = b.mode === "listening";
  const starting = b.mode === "starting";
  const thinking = b.mode === "thinking" || (b.mode === "answer" && !body);

  return (
    <div className="pointer-events-none fixed inset-0 select-none">
      {spot && (
        <>
          <span className="point-ring absolute h-14 w-14 rounded-full border-[3px] border-[#fbbf24]" style={{ left: `calc(${spot.fx * 100}% - 28px)`, top: `calc(${spot.fy * 100}% - 28px)` }} />
          <svg
            key={`${spot.fx},${spot.fy}`}
            className="buddy-fly absolute h-7 w-7 drop-shadow-[0_0_10px_rgba(251,191,36,0.8)]"
            viewBox="0 0 24 24"
            style={{ "--from-x": `${(from?.[0] ?? spot.fx) * 100}vw`, "--from-y": `${(from?.[1] ?? spot.fy) * 100}vh`, left: `${spot.fx * 100}%`, top: `${spot.fy * 100}%` } as React.CSSProperties}
          >
            <path d="M3 2l16 9-7 1.6L8.6 20z" fill="#fbbf24" stroke="#1a1204" strokeWidth="1.2" strokeLinejoin="round" />
          </svg>
        </>
      )}

      <div
        className={`buddy-card absolute w-[328px] overflow-hidden rounded-[22px] bg-[#0b0b0c]/[0.94] text-white backdrop-blur-xl ${leaving ? "buddy-out" : "buddy-pop"} ${listening ? "buddy-glow" : ""}`}
        style={{ left, top }}
      >
        <div className="px-4 py-3.5">
          {/* What you said: live while listening, small once answered. */}
          {(listening || b.said) && (
            <div className={`flex items-start gap-2.5 transition-all duration-300 ${listening ? "" : "mb-2"}`}>
              {listening && (
                <span className="mt-[3px] flex h-4 shrink-0 items-end gap-[3px]">
                  {[0, 1, 2, 3, 4].map((i) => <span key={i} className="eq w-[3px] rounded-full bg-[#fbbf24]" style={{ animationDelay: `${i * 0.11}s` }} />)}
                </span>
              )}
              <p className={`min-w-0 transition-all duration-300 ${listening ? "text-[14px] font-medium leading-snug text-white" : "truncate text-[11.5px] text-white/45"}`}>
                {listening ? (b.said || <span className="text-white/50">Listening…</span>) : `“${b.said}”`}
              </p>
            </div>
          )}

          {starting && (
            <p className="flex items-center gap-2 text-[13px] text-white/60">
              <span className="flex gap-1">{[0, 1, 2].map((i) => <span key={i} className="dot h-1.5 w-1.5 rounded-full bg-[#fbbf24]/80" style={{ animationDelay: `${i * 0.15}s` }} />)}</span>
              Mic starting · speak after the chime
            </p>
          )}
          {!listening && !starting && (thinking ? (
            <p className="flex items-center gap-2 text-[13px] text-white/60">
              <span className="flex gap-1">{[0, 1, 2].map((i) => <span key={i} className="dot h-1.5 w-1.5 rounded-full bg-white/70" style={{ animationDelay: `${i * 0.15}s` }} />)}</span>
              {b.step > 1 ? `Looking for step ${b.step}` : "Thinking"}
            </p>
          ) : (
            <div className="fade-in">
              <p className={`max-h-[230px] overflow-hidden whitespace-pre-wrap text-[14px] leading-[1.45] ${b.mode === "error" ? "text-[#fca5a5]" : ""}`}>
                {b.mode === "guide" && <span className="mr-2 inline-flex h-5 min-w-5 items-center justify-center rounded-full bg-[#fbbf24] px-1.5 align-[1px] text-[11px] font-bold text-black">{b.step}</span>}
                {b.mode === "done" && <span className="mr-1.5 text-[#4ade80]">✓</span>}
                {spoken >= 0 && b.mode !== "error" ? (
                  <><span>{body.slice(0, spoken)}</span><span className="text-white/40 transition-colors">{body.slice(spoken)}</span></>
                ) : body}
              </p>
              {(b.mode === "guide" || b.mode === "answer") && (
                <p className="mt-2 text-[10.5px] text-white/35">
                  {b.mode === "guide" ? `Click it and I'll show the next step · tap ${KEY} to stop` : <Tip />}
                </p>
              )}
            </div>
          ))}
          {listening && <p className="mt-1.5 pl-[30px] text-[10.5px] text-white/35">Let go when you're done</p>}
        </div>
      </div>
    </div>
  );
}

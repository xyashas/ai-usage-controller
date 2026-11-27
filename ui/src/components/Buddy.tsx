import { useEffect, useRef, useState } from "react";
import { api, onAsk, onBuddy } from "../api";
import type { BuddyState } from "../types";

/** The see-through window over your screen: a listening pill, answers in a
 *  bubble by your cursor, and a little pointer that flies to what to click. */
export function Buddy() {
  const [b, setB] = useState<BuddyState | null>(null);
  const [stream, setStream] = useState("");
  const [from, setFrom] = useState<[number, number] | null>(null);
  const idRef = useRef("");
  const fade = useRef<number | undefined>(undefined);

  useEffect(() => {
    document.documentElement.style.background = "transparent";
    document.body.style.background = "transparent";
    const apply = (s: BuddyState) => {
      clearTimeout(fade.current);
      if (s.id !== idRef.current) setStream("");
      idRef.current = s.id;
      setB((prev) => {
        // The pointer flies from where it was (or your cursor) to the new spot.
        setFrom(prev?.spots[0] ? [prev.spots[0].fx, prev.spots[0].fy] : s.cursor);
        return s;
      });
      if (s.speak && "speechSynthesis" in window) {
        speechSynthesis.cancel();
        const u = new SpeechSynthesisUtterance(s.speak.replace(/[*`#]/g, "").slice(0, 600));
        u.onend = () => { if (s.mode === "answer" || s.mode === "done") fade.current = window.setTimeout(hide, 9000); };
        speechSynthesis.speak(u);
      }
      const wait = s.mode === "error" ? 9000 : s.mode === "done" ? 7000 : s.mode === "answer" && !s.speak ? 0 : s.mode === "guide" ? 120000 : 0;
      if (wait) fade.current = window.setTimeout(hide, wait);
    };
    api.buddyState().then((s) => s && apply(s), () => {});
    const subs = [
      onBuddy((s) => (s.mode === "hide" ? setB(null) : apply(s))),
      onAsk((id, piece) => { if (id === idRef.current) setStream((t) => t + piece); }, () => {}),
    ];
    return () => { subs.forEach((p) => p.then((u) => u())); };
  }, []);

  function hide() { setB(null); speechSynthesis?.cancel(); api.buddyHide(); }

  if (!b) return null;
  const spot = b.spots[0];
  // The bubble sits by the pointer (guide) or by your cursor, kept on screen.
  const [ax, ay] = spot ? [spot.fx, spot.fy] : b.cursor;
  const left = ax > 0.7 ? `calc(${ax * 100}% - 340px)` : `calc(${ax * 100}% + 26px)`;
  const top = ay > 0.75 ? `calc(${ay * 100}% - 150px)` : `calc(${ay * 100}% + 22px)`;
  const text = b.mode === "answer" && !b.text ? stream : b.text;

  return (
    <div className="buddy pointer-events-none fixed inset-0 select-none">
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
      <div className="buddy-pop absolute w-[320px] rounded-2xl bg-[#0b0b0c]/95 px-3.5 py-3 text-[13px] leading-snug text-white shadow-[0_10px_40px_rgba(0,0,0,0.5)] ring-1 ring-white/10" style={{ left, top }}>
        {b.mode === "listening" ? (
          <div className="flex items-center gap-2.5">
            <span className="flex h-4 items-end gap-[3px]">{[0, 1, 2, 3].map((i) => <span key={i} className="eq w-[3px] rounded-full bg-[#fbbf24]" style={{ animationDelay: `${i * 0.12}s` }} />)}</span>
            <span className="font-medium">Listening… let go when you're done</span>
          </div>
        ) : (
          <div className="space-y-1.5">
            {b.said && <p className="truncate text-[11.5px] text-white/50">“{b.said}”</p>}
            {b.mode === "thinking" ? (
              <p className="flex items-center gap-2 text-white/70"><span className="live-dot h-1.5 w-1.5 rounded-full bg-[#fbbf24]" />{b.step > 1 ? `Looking for step ${b.step}…` : "Thinking…"}</p>
            ) : (
              <p className={`max-h-[220px] overflow-hidden whitespace-pre-wrap ${b.mode === "error" ? "text-[#fca5a5]" : ""}`}>
                {b.mode === "guide" && <span className="mr-1.5 rounded-md bg-[#fbbf24] px-1.5 py-px text-[11px] font-bold text-black">{b.step}</span>}
                {b.mode === "done" && "✓ "}
                {(text || "…").replace(/\*\*/g, "")}
              </p>
            )}
            {b.mode === "guide" && <p className="text-[10.5px] text-white/40">Click it and I'll show the next step · tap {navigator.userAgent.includes("Mac") ? "⌥⇧Space" : "Alt+Shift+Space"} to stop</p>}
            {b.mode === "answer" && <p className="text-[10.5px] text-white/40">Tap {navigator.userAgent.includes("Mac") ? "⌥⇧Space" : "Alt+Shift+Space"} to continue in the panel</p>}
          </div>
        )}
      </div>
    </div>
  );
}

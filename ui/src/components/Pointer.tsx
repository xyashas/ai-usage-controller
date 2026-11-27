import { useEffect, useState } from "react";
import { api, onPoints } from "../api";

type Spot = { fx: number; fy: number; label: string };

/** The see-through window over the screen: rings and labels where to click. */
export function Pointer() {
  const [spots, setSpots] = useState<Spot[]>([]);
  useEffect(() => {
    document.documentElement.style.background = "transparent";
    document.body.style.background = "transparent";
    api.askPoints().then((s) => s && setSpots(s), () => {});
    const sub = onPoints(setSpots);
    return () => { sub.then((u) => u()); };
  }, []);
  // Fade away after a while; the Ask panel can also hide them.
  useEffect(() => {
    if (!spots.length) return;
    const t = setTimeout(() => { setSpots([]); api.askHidePoints(); }, 20000);
    return () => clearTimeout(t);
  }, [spots]);

  return (
    <div className="pointer-events-none fixed inset-0">
      {spots.map((s, i) => (
        <div key={i} className="absolute" style={{ left: `${s.fx * 100}%`, top: `${s.fy * 100}%` }}>
          <span className="point-ring absolute -left-6 -top-6 h-12 w-12 rounded-full border-[3px] border-[#fbbf24]" style={{ animationDelay: `${i * 0.25}s` }} />
          <span className="absolute -left-1.5 -top-1.5 h-3 w-3 rounded-full bg-[#fbbf24] shadow-[0_0_12px_rgba(251,191,36,0.9)]" />
          <span className="absolute left-7 top-[-11px] whitespace-nowrap rounded-full bg-black/85 px-2.5 py-1 text-[12px] font-semibold text-white shadow-lg">
            {spots.length > 1 ? `${i + 1}. ` : ""}{s.label || "Click here"}
          </span>
        </div>
      ))}
    </div>
  );
}

import { useCallback, useEffect, useRef, useState } from "react";
import type { Mode, Snapshot } from "./types";
import { api, inTauri, onOpenView, onSnapshot, onToggle } from "./api";
import { Collapsed } from "./components/Collapsed";
import { Expanded } from "./components/Expanded";
import { Paywall } from "./components/Paywall";
import { AuditCard } from "./components/AuditCard";
import { SettingsView } from "./components/SettingsView";
import { Onboarding } from "./components/Onboarding";

type View = "panel" | "settings" | "paywall" | "audit" | "onboarding";

// Window sizes must match app/src-tauri/src/window.rs
export const SIZES = {
  collapsed: { macos: [210, 32], windows: [230, 34], linux: [230, 34] },
  expanded: [380, 480],
} as const;

export default function App() {
  const [snap, setSnap] = useState<Snapshot | null>(null);
  const [mode, setModeState] = useState<Mode>("collapsed");
  const [view, setView] = useState<View>("panel");
  const [platform, setPlatform] = useState<"macos" | "windows" | "linux">("macos");
  const [notch, setNotch] = useState(0);
  const leaveTimer = useRef<number | null>(null);
  const panelRef = useRef<HTMLDivElement | null>(null);

  const refresh = useCallback(async () => setSnap(await api.snapshot()), []);

  const setMode = useCallback(async (m: Mode) => {
    setModeState(m);
    await api.setMode(m);
    if (m === "collapsed") setView((v) => (v === "onboarding" ? v : "panel"));
  }, []);

  useEffect(() => {
    api.platform().then(setPlatform);
    api.layout().then((l) => l && setNotch(l.notch_height));
    refresh();
    const subs = [
      onSnapshot(setSnap),
      onToggle(() => setModeState((m) => {
        const next = m === "collapsed" ? "expanded" : "collapsed";
        api.setMode(next);
        return next;
      })),
      onOpenView((v) => {
        setView(v as View);
        setModeState("expanded");
        api.setMode("expanded");
      }),
    ];
    if (!inTauri) document.body.classList.add("preview");
    const onBlur = () => setModeState((m) => {
      if (m === "expanded") api.setMode("collapsed");
      return "collapsed";
    });
    window.addEventListener("blur", onBlur);
    return () => {
      subs.forEach((p) => p.then((u) => u()));
      window.removeEventListener("blur", onBlur);
    };
  }, [refresh]);

  // First run: open straight into onboarding.
  useEffect(() => {
    if (snap && !snap.onboarded && view !== "onboarding") {
      setView("onboarding");
      setMode("expanded");
    }
  }, [snap, view, setMode]);

  // Escalating alerts pop the panel open once.
  const lastAlert = useRef<string | null>(null);
  useEffect(() => {
    const a = snap?.alerts[0];
    if (a && a.id !== lastAlert.current && a.level === "red" && mode === "collapsed") {
      lastAlert.current = a.id;
      setMode("expanded");
    }
  }, [snap, mode, setMode]);

  // Fit the expanded window to the panel's content.
  useEffect(() => {
    if (mode !== "expanded" || !panelRef.current || !inTauri) return;
    const el = panelRef.current;
    const ro = new ResizeObserver(() => {
      const h = Math.ceil(el.scrollHeight);
      if (h > 0) api.setMode("expanded", h);
    });
    ro.observe(el);
    return () => ro.disconnect();
  }, [mode, view]);

  if (!snap) return null;

  const onLeave = () => {
    if (view !== "panel") return; // keep forms open while the user is typing
    leaveTimer.current = window.setTimeout(() => setMode("collapsed"), 450);
  };
  const onEnter = () => {
    if (leaveTimer.current) clearTimeout(leaveTimer.current);
  };

  const [cw, ch] = SIZES.collapsed[platform];
  const [ew, eh] = SIZES.expanded;
  const fit = view === "panel" && mode === "expanded";
  const frame = inTauri
    ? { width: "100%", height: fit ? "auto" : "100%" }
    : mode === "collapsed"
      ? { width: cw, height: ch }
      : { width: ew, height: fit ? "auto" : eh, maxHeight: 560 };

  return (
    <div className={inTauri ? "h-full w-full" : "flex h-full w-full justify-center pt-0"}>
      <div ref={panelRef} style={frame} onMouseLeave={mode === "expanded" ? onLeave : undefined} onMouseEnter={onEnter}>
        {mode === "collapsed" ? (
          <Collapsed snap={snap} platform={platform} notch={notch} onOpen={() => setMode("expanded")} />
        ) : view === "onboarding" ? (
          <Onboarding onDone={async () => { await refresh(); setView("panel"); }} />
        ) : view === "settings" ? (
          <SettingsView snap={snap} onBack={() => { setView("panel"); refresh(); }} onPaywall={() => setView("paywall")} />
        ) : view === "paywall" ? (
          <Paywall snap={snap} onBack={() => setView("panel")} refresh={refresh} />
        ) : view === "audit" ? (
          <AuditCard snap={snap} onBack={() => setView("panel")} refresh={refresh} />
        ) : (
          <Expanded snap={snap} onView={(v) => setView(v)} refresh={refresh} />
        )}
      </div>
    </div>
  );
}

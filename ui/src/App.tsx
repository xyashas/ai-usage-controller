import { useCallback, useEffect, useLayoutEffect, useRef, useState } from "react";
import type { Snapshot } from "./types";
import { api, inTauri, onHover, onOpenView, onSnapshot, onToggle } from "./api";
import { Collapsed } from "./components/Collapsed";
import { Expanded } from "./components/Expanded";
import { Paywall } from "./components/Paywall";
import { AuditCard } from "./components/AuditCard";
import { SettingsView } from "./components/SettingsView";
import { Onboarding } from "./components/Onboarding";
import { CheckSetup } from "./components/CheckSetup";

export type View = "panel" | "settings" | "paywall" | "audit" | "onboarding" | "setup";
type Platform = "macos" | "windows" | "linux";

const PANEL_W = 360;
const MAX_H = 560;
const OPEN_DELAY = 140; // hover this long before the panel opens (stops drive-by opens)
const CLOSE_DELAY = 260; // grace period after the cursor leaves
const ANIM_MS = 380;

function pillSize(platform: Platform, notch: number) {
  if (platform === "macos" && notch > 0) return { w: 236, h: notch + 22, top: 0 };
  if (platform === "macos") return { w: 210, h: 28, top: 0 };
  return { w: 206, h: 32, top: 6 };
}

export default function App() {
  const [snap, setSnap] = useState<Snapshot | null>(null);
  const [open, setOpenState] = useState(false);
  const [mounted, setMounted] = useState(false); // panel content stays mounted while closing
  const [view, setView] = useState<View>("panel");
  const [platform, setPlatform] = useState<Platform>("windows");
  const [notch, setNotch] = useState(0);
  const [contentH, setContentH] = useState(320);

  const openRef = useRef(false);
  const hoverRef = useRef(false); // cursor is over the widget's shape
  const viewRef = useRef<View>("panel");
  const hoverTimer = useRef<number | undefined>(undefined);
  const unmountTimer = useRef<number | undefined>(undefined);
  const surfaceRef = useRef<HTMLDivElement | null>(null);
  const contentRef = useRef<HTMLDivElement | null>(null);
  const innerRef = useRef<HTMLDivElement | null>(null); // stable wrapper around the current view
  viewRef.current = view;

  const refresh = useCallback(async () => setSnap(await api.snapshot()), []);

  const setOpen = useCallback((next: boolean) => {
    if (openRef.current === next) return;
    openRef.current = next;
    clearTimeout(unmountTimer.current);
    if (next) {
      setMounted(true);
    } else {
      unmountTimer.current = window.setTimeout(() => {
        // Keep onboarding mounted so a half-finished setup isn't lost.
        if (viewRef.current !== "onboarding") setMounted(false);
        // Back to the main panel next time, unless setup isn't finished.
        setView((v) => (v === "onboarding" ? v : "panel"));
      }, ANIM_MS);
    }
    setOpenState(next);
  }, []);

  // Forms stay open when the cursor wanders off; they close on click-away or Esc.
  const sticky = () => viewRef.current !== "panel";

  const onHoverChange = useCallback(
    (inside: boolean) => {
      hoverRef.current = inside;
      clearTimeout(hoverTimer.current);
      if (inside && !openRef.current) {
        hoverTimer.current = window.setTimeout(() => setOpen(true), OPEN_DELAY);
      } else if (!inside && openRef.current && !sticky()) {
        hoverTimer.current = window.setTimeout(() => setOpen(false), CLOSE_DELAY);
      }
    },
    [setOpen],
  );

  useEffect(() => {
    api.platform().then((p) => setPlatform(p as Platform));
    api.layout().then((l) => l && setNotch(l.notch_height));
    refresh();
    const subs = [
      onSnapshot(setSnap),
      onHover(onHoverChange),
      onToggle(() => setOpen(!openRef.current)),
      onOpenView((v) => {
        setView(v as View);
        setOpen(true);
      }),
    ];
    if (!inTauri) document.body.classList.add("preview");
    // Clicking another app closes the panel. Focus can also shift while the
    // cursor is still on the panel (e.g. the first click into it), so only
    // treat it as "clicked away" when the cursor is elsewhere.
    const onBlur = () => {
      if (hoverRef.current) return;
      clearTimeout(hoverTimer.current);
      setOpen(false);
    };
    const onKey = (e: KeyboardEvent) => e.key === "Escape" && setOpen(false);
    window.addEventListener("blur", onBlur);
    window.addEventListener("keydown", onKey);
    return () => {
      subs.forEach((p) => p.then((u) => u()));
      window.removeEventListener("blur", onBlur);
      window.removeEventListener("keydown", onKey);
    };
  }, [refresh, onHoverChange, setOpen]);

  // First run: open straight into onboarding.
  useEffect(() => {
    if (snap && !snap.onboarded && view !== "onboarding") {
      setView("onboarding");
      setOpen(true);
    }
  }, [snap, view, setOpen]);

  // A red alert pops the panel open once.
  const lastAlert = useRef<string | null>(null);
  useEffect(() => {
    const a = snap?.alerts[0];
    if (a && !snap?.idle_hidden && a.id !== lastAlert.current && a.level === "red" && !openRef.current) {
      lastAlert.current = a.id;
      setOpen(true);
    }
  }, [snap, setOpen]);

  // The surface's height follows its content, so it can animate between sizes.
  // Measures a wrapper that always holds the current view, so every change in
  // content size (data loading, switching views, fonts) is caught.
  useLayoutEffect(() => {
    const el = innerRef.current;
    if (!mounted || !el) return;
    const measure = () => setContentH(Math.min(MAX_H, Math.ceil(el.getBoundingClientRect().height)));
    measure();
    const ro = new ResizeObserver(measure);
    ro.observe(el);
    return () => ro.disconnect();
  }, [mounted]);

  // Tell the native side which area takes the mouse (everything else clicks
  // through). Uses the shape's target size rather than measuring mid-animation,
  // so a click lands on the panel the moment it starts to open.
  const pillNow = pillSize(platform, notch);
  const hidden = !!snap?.idle_hidden && !open;
  const targetW = open ? PANEL_W : pillNow.w;
  const targetH = open ? contentH : pillNow.h;
  useEffect(() => {
    if (!inTauri) return;
    const send = () => api.setHitRect({ x: (window.innerWidth - targetW) / 2, y: pillNow.top, w: hidden ? 0 : targetW, h: hidden ? 0 : targetH });
    send();
    window.addEventListener("resize", send);
    return () => window.removeEventListener("resize", send);
  }, [targetW, targetH, pillNow.top, hidden]);

  if (!snap) return null;

  const pill = pillSize(platform, notch);
  const mac = platform === "macos";
  const height = open ? contentH : pill.h;
  const radius = open ? (mac ? "0 0 28px 28px" : "24px") : mac ? `0 0 ${Math.min(16, pill.h / 2)}px ${Math.min(16, pill.h / 2)}px` : `${pill.h / 2}px`;
  const alert = snap.alerts[0];

  const goto = (v: View) => {
    setView(v);
    if (v !== "panel") api.focus();
  };

  return (
    <div className="relative h-full w-full">
      <div
        ref={surfaceRef}
        onMouseEnter={inTauri ? undefined : () => onHoverChange(true)}
        onMouseLeave={inTauri ? undefined : () => onHoverChange(false)}
        className={`surface absolute left-1/2 overflow-hidden ${open ? "is-open" : ""} ${!open && alert ? (alert.level === "red" ? "pulse-red" : "glow-amber") : ""}`}
        aria-hidden={hidden}
        inert={hidden}
        style={{ opacity: hidden ? 0 : 1, pointerEvents: hidden ? "none" : undefined, top: pill.top, width: open ? PANEL_W : pill.w, height, borderRadius: radius, transform: "translateX(-50%)" }}
      >
        <div className="pill-layer absolute inset-x-0 bottom-0" style={{ height: mac && notch ? 22 : pill.h, opacity: open ? 0 : 1 }}>
          <Collapsed snap={snap} onOpen={() => setOpen(true)} />
        </div>
        {mounted && (
          <div
            ref={contentRef}
            className="panel-layer absolute left-0 top-0 overflow-y-auto"
            style={{ width: PANEL_W, maxHeight: MAX_H, opacity: open ? 1 : 0 }}
          >
            <div ref={innerRef} style={{ paddingTop: mac ? notch : 0 }}>
            {view === "onboarding" ? (
              <Onboarding
                onDone={async () => {
                  await refresh();
                  setView("panel");
                }}
              />
            ) : view === "settings" ? (
              <SettingsView snap={snap} onBack={() => { setView("panel"); refresh(); }} onPaywall={() => goto("paywall")} onSetup={() => goto("setup")} />
            ) : view === "paywall" ? (
              <Paywall snap={snap} onBack={() => setView("panel")} refresh={refresh} />
            ) : view === "audit" ? (
              <AuditCard snap={snap} onBack={() => setView("panel")} refresh={refresh} />
            ) : view === "setup" ? (
              <CheckSetup onBack={() => setView("panel")} />
            ) : (
              <Expanded snap={snap} onView={goto} refresh={refresh} />
            )}
            </div>
          </div>
        )}
      </div>
    </div>
  );
}

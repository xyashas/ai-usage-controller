import { useEffect, useState } from "react";
import type { View } from "../App";
import { api, onInboxChanged } from "../api";
import { GearIcon, Logo, PulseIcon, SparkIcon } from "./icons";

function InboxIcon() {
  return (
    <svg width="15" height="15" viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.45" strokeLinecap="round" strokeLinejoin="round">
      <path d="M2.5 9.5h3l1 1.75h3l1-1.75h3" />
      <path d="M3.4 3.5h9.2a1 1 0 0 1 .98.8l.92 5.2v3a1 1 0 0 1-1 1h-11a1 1 0 0 1-1-1v-3l.92-5.2a1 1 0 0 1 .98-.8Z" />
    </svg>
  );
}

/** The panel's header: name, the Usage / Agents / Handoff switch, inbox badge, setup and settings. */
export function TopBar({ tab, onView }: { tab: "panel" | "agents" | "handoffs" | "inbox"; onView: (v: View) => void }) {
  const [inboxCount, setInboxCount] = useState(0);

  useEffect(() => {
    const load = () => api.inboxList().then((items) => setInboxCount(items.filter((i) => !i.done).length), () => {});
    load();
    const unsub = onInboxChanged(load);
    return () => {
      unsub.then((u) => u());
    };
  }, []);

  return (
    <header className="flex items-center gap-2 px-1 pb-0.5 pt-0.5">
      <span className="flex items-center gap-2 text-[13px] font-semibold tracking-tight">
        <Logo />
        AI Usage
      </span>
      <div className="seg ml-auto" role="group" aria-label="Section">
        <button aria-pressed={tab === "panel"} onClick={() => onView("panel")}>Usage</button>
        <button aria-pressed={tab === "agents"} onClick={() => onView("agents")}>Agents</button>
        <button aria-pressed={tab === "handoffs"} onClick={() => onView("handoffs")}>Handoff</button>
      </div>
      <button
        title="Inbox"
        aria-label={inboxCount > 0 ? `Inbox (${inboxCount})` : "Inbox"}
        aria-pressed={tab === "inbox"}
        className={`icon-btn relative ${tab === "inbox" ? "bg-white/[0.14] text-fg" : ""}`}
        onClick={() => onView("inbox")}
      >
        <InboxIcon />
        {inboxCount > 0 && (
          <span className="tabular absolute -right-0.5 -top-0.5 flex h-3.5 min-w-[14px] items-center justify-center rounded-full bg-fg px-1 text-[9px] font-bold leading-none text-black">
            {inboxCount > 9 ? "9+" : inboxCount}
          </span>
        )}
      </button>
      <button title={`Ask (${navigator.userAgent.includes("Mac") ? "⌥⇧Space" : "Alt+Shift+Space"})`} aria-label="Ask" className="icon-btn" onClick={() => api.askForget("all").finally(() => onView("ask"))}><SparkIcon /></button>
      <button title="Check setup" aria-label="Check setup" className="icon-btn" onClick={() => onView("setup")}><PulseIcon /></button>
      <button title="Settings" aria-label="Settings" className="icon-btn" onClick={() => onView("settings")}><GearIcon /></button>
    </header>
  );
}

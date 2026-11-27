import type { View } from "../App";
import { GearIcon, Logo, PulseIcon } from "./icons";

/** The panel's header: name, the Usage / Agents / Handoff switch, setup and settings. */
export function TopBar({ tab, onView }: { tab: "panel" | "agents" | "handoffs"; onView: (v: View) => void }) {
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
      <button title="Check setup" aria-label="Check setup" className="icon-btn" onClick={() => onView("setup")}><PulseIcon /></button>
      <button title="Settings" aria-label="Settings" className="icon-btn" onClick={() => onView("settings")}><GearIcon /></button>
    </header>
  );
}

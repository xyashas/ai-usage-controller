import { useEffect, useRef, useState } from "react";
import type { Snapshot, Suggestion } from "../types";
import { onSuggestion } from "../api";
import { CloseIcon } from "./icons";

const SHOW_MS = 8000;

/**
 * Slides down from the pill for 8 s (pauses while hovered) when an error is
 * copied, tests fail, or a limit gets tight mid-task.
 */
export function SuggestionChip({ snap, open, top }: { snap: Snapshot; open: boolean; top: number }) {
  const [active, setActive] = useState<Suggestion | null>(null);
  const [hovered, setHovered] = useState(false);
  const seen = useRef(new Set<string>());
  const timer = useRef<number | undefined>(undefined);

  // Listen to clipboard-driven suggestions emitted from Rust.
  useEffect(() => {
    const unsub = onSuggestion((s) => {
      if (seen.current.has(s.id)) return;
      seen.current.add(s.id);
      setActive(s);
    });
    return () => {
      unsub.then((u) => u());
    };
  }, []);

  // Pick up snapshot-driven suggestions (failed tests, limit mid-task).
  useEffect(() => {
    const next = (snap.suggestions ?? []).find((s) => !seen.current.has(s.id));
    if (next) {
      seen.current.add(next.id);
      setActive(next);
    }
  }, [snap.suggestions]);

  // Auto-hide after 8 s unless hovered.
  useEffect(() => {
    clearTimeout(timer.current);
    if (!active || hovered) return;
    timer.current = window.setTimeout(() => setActive(null), SHOW_MS);
    return () => clearTimeout(timer.current);
  }, [active, hovered]);

  if (!active || open || snap.idle_hidden) return null;

  function trigger() {
    if (!active) return;
    const detail: Record<string, unknown> = {
      action: active.action,
      text:
        active.action === "ask_fix"
          ? "Fix the error on my clipboard"
          : active.action === "send_failure"
          ? "Fix the failing tests"
          : active.detail,
    };
    if (active.kind.kind === "limit_mid_task") {
      detail.tool = active.kind.tool;
      detail.to = active.kind.to;
    } else if (active.kind.kind === "tests_failed") {
      detail.task_id = active.kind.task_id;
    }
    window.dispatchEvent(new CustomEvent("aiuc-ask", { detail }));
    setActive(null);
  }

  return (
    <div
      role="status"
      onMouseEnter={() => setHovered(true)}
      onMouseLeave={() => setHovered(false)}
      className="fade-in absolute left-1/2 z-20 flex items-center gap-2 rounded-full bg-black px-3 py-1.5 text-[12px] text-fg shadow-[0_12px_32px_rgba(0,0,0,0.65)] ring-1 ring-white/10"
      style={{ top, transform: "translateX(-50%)" }}
    >
      <span className="max-w-[200px] truncate font-medium">{active.title}</span>
      <button type="button" onClick={trigger} className="btn btn-primary !px-2.5 !py-0.5 text-[11px]">
        {active.action === "handoff" ? "Hand off" : "Fix"}
      </button>
      <button
        type="button"
        aria-label="Dismiss suggestion"
        onClick={() => setActive(null)}
        className="icon-btn -mr-1 !h-5 !w-5"
      >
        <CloseIcon />
      </button>
    </div>
  );
}

const base = { width: 16, height: 16, viewBox: "0 0 24 24", fill: "none", stroke: "currentColor", strokeWidth: 1.8, strokeLinecap: "round" as const, strokeLinejoin: "round" as const };

export function GearIcon() {
  return (
    <svg {...base} aria-hidden>
      <circle cx="12" cy="12" r="3" />
      <path d="M19.4 15a1.7 1.7 0 0 0 .3 1.8l.1.1a2 2 0 1 1-2.8 2.8l-.1-.1a1.7 1.7 0 0 0-1.8-.3 1.7 1.7 0 0 0-1 1.5V21a2 2 0 1 1-4 0v-.1a1.7 1.7 0 0 0-1.1-1.5 1.7 1.7 0 0 0-1.8.3l-.1.1a2 2 0 1 1-2.8-2.8l.1-.1a1.7 1.7 0 0 0 .3-1.8 1.7 1.7 0 0 0-1.5-1H3a2 2 0 1 1 0-4h.1a1.7 1.7 0 0 0 1.5-1.1 1.7 1.7 0 0 0-.3-1.8l-.1-.1a2 2 0 1 1 2.8-2.8l.1.1a1.7 1.7 0 0 0 1.8.3H9a1.7 1.7 0 0 0 1-1.5V3a2 2 0 1 1 4 0v.1a1.7 1.7 0 0 0 1 1.5 1.7 1.7 0 0 0 1.8-.3l.1-.1a2 2 0 1 1 2.8 2.8l-.1.1a1.7 1.7 0 0 0-.3 1.8V9a1.7 1.7 0 0 0 1.5 1H21a2 2 0 1 1 0 4h-.1a1.7 1.7 0 0 0-1.5 1z" />
    </svg>
  );
}

export function PulseIcon() {
  return (
    <svg {...base} aria-hidden>
      <path d="M3 12h4l2.5-6 5 12 2.5-6H21" />
    </svg>
  );
}

export function BackIcon() {
  return (
    <svg {...base} width={14} height={14} aria-hidden>
      <path d="M15 18l-6-6 6-6" />
    </svg>
  );
}

export function Check({ ok }: { ok: boolean | null }) {
  if (ok == null) return <span className="inline-block h-4 w-4 rounded-full border border-fg/20" aria-hidden />;
  return ok ? (
    <span className="flex h-4 w-4 items-center justify-center rounded-full bg-ok/20 text-ok" aria-label="OK">
      <svg width="10" height="10" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="3.5" strokeLinecap="round" strokeLinejoin="round"><path d="M5 12l5 5 9-10" /></svg>
    </span>
  ) : (
    <span className="flex h-4 w-4 items-center justify-center rounded-full bg-warn/20 text-warn" aria-label="Needs attention">
      <svg width="10" height="10" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="3.5" strokeLinecap="round"><path d="M12 7v6M12 17h.01" /></svg>
    </span>
  );
}

export function SwapIcon() {
  return (
    <svg {...base} width={14} height={14} aria-hidden>
      <path d="M4 8h14l-4-4M20 16H6l4 4" />
    </svg>
  );
}

export function PlusIcon() {
  return (
    <svg {...base} width={14} height={14} aria-hidden>
      <path d="M12 5v14M5 12h14" />
    </svg>
  );
}

export function RefreshIcon({ spin = false }: { spin?: boolean }) {
  return (
    <svg {...base} width={14} height={14} aria-hidden className={spin ? "animate-spin" : undefined}>
      <path d="M20 11a8 8 0 1 0-2.3 5.7M20 4v7h-7" />
    </svg>
  );
}

export function ArrowIcon() {
  return (
    <svg {...base} width={12} height={12} strokeWidth={2.2} aria-hidden>
      <path d="M7 17 17 7M8 7h9v9" />
    </svg>
  );
}

export function BranchIcon() {
  return (
    <svg {...base} width={12} height={12} aria-hidden>
      <circle cx="6" cy="5" r="2" /><circle cx="6" cy="19" r="2" /><circle cx="18" cy="7" r="2" />
      <path d="M6 7v10M18 9c0 5-6 4-12 8" />
    </svg>
  );
}

export function CloseIcon() {
  return (
    <svg {...base} width={14} height={14} aria-hidden>
      <path d="M6 6l12 12M18 6 6 18" />
    </svg>
  );
}

/** The app's mark: a small gauge. */
export function Logo() {
  return (
    <svg width="18" height="18" viewBox="0 0 24 24" aria-hidden>
      <rect x="1" y="1" width="22" height="22" rx="7" fill="#F1D9CB" />
      <path d="M6.5 15.5a5.5 5.5 0 0 1 11 0" fill="none" stroke="#2A1810" strokeWidth="2.2" strokeLinecap="round" />
      <path d="M12 15.5l3-4" stroke="#C4673F" strokeWidth="2.2" strokeLinecap="round" />
    </svg>
  );
}

export function MicIcon() {
  return (
    <svg {...base} width={15} height={15} aria-hidden>
      <rect x="9" y="3" width="6" height="11" rx="3" />
      <path d="M5.5 11a6.5 6.5 0 0 0 13 0M12 17.5V21" />
    </svg>
  );
}

export function SparkIcon() {
  return (
    <svg {...base} aria-hidden>
      <path d="M12 3l1.8 5.2L19 10l-5.2 1.8L12 17l-1.8-5.2L5 10l5.2-1.8z" />
      <path d="M19 15l.8 2.2L22 18l-2.2.8L19 21l-.8-2.2L16 18l2.2-.8z" />
    </svg>
  );
}

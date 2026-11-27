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

import { useCallback, useState } from "react";
import { api } from "../api";

/** Token-level comparison highlighting newly added words in the polished prompt. */
function highlightAdded(before: string, after: string): { text: string; added: boolean }[] {
  const beforeWords = new Set(
    before
      .toLowerCase()
      .split(/\s+/)
      .map((w) => w.replace(/^[^\w]+|[^\w]+$/g, ""))
      .filter(Boolean),
  );
  return after.split(/(\s+)/).map((part) => {
    if (/^\s+$/.test(part)) return { text: part, added: false };
    const norm = part.toLowerCase().replace(/^[^\w]+|[^\w]+$/g, "");
    return { text: part, added: !!norm && !beforeWords.has(norm) };
  });
}

export function usePromptPolish(text: string, repo: string | null) {
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const [draft, setDraft] = useState<{ original: string; polished: string } | null>(null);

  const polish = useCallback(async () => {
    const raw = text.trim();
    if (!raw || busy) return;
    setBusy(true);
    setError("");
    try {
      const polished = await api.polishPrompt(raw, repo);
      if (polished && polished.trim() && polished.trim() !== raw) {
        setDraft({ original: raw, polished: polished.trim() });
      } else {
        setDraft(null);
      }
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  }, [text, repo, busy]);

  const clear = useCallback(() => {
    setDraft(null);
    setError("");
  }, []);

  return { busy, error, draft, polish, clear };
}

export function PolishButton({ disabled, busy, onClick }: { disabled: boolean; busy: boolean; onClick: () => void }) {
  return (
    <button
      type="button"
      disabled={disabled || busy}
      onClick={onClick}
      title="Rewrite prompt with repo context"
      className="btn !px-3 !py-1.5 text-[12px]"
    >
      {busy ? "Polishing…" : "Polish"}
    </button>
  );
}

export function PolishDiffCard({
  original,
  polished,
  onAccept,
  onKeep,
}: {
  original: string;
  polished: string;
  onAccept: (next: string) => void;
  onKeep: () => void;
}) {
  const tokens = highlightAdded(original, polished);
  return (
    <section className="tile fade-in space-y-2.5" aria-label="Polished prompt">
      <div className="flex items-center justify-between gap-2">
        <span className="eyebrow">Polished prompt</span>
        <span className="text-[10.5px] text-muted">Added context highlighted</span>
      </div>
      <p className="select-text whitespace-pre-wrap text-[12.5px] leading-relaxed text-fg/90">
        {tokens.map((t, i) =>
          t.added ? (
            <mark key={i} className="rounded bg-[#a5b4fc]/20 px-0.5 text-fg">
              {t.text}
            </mark>
          ) : (
            <span key={i}>{t.text}</span>
          ),
        )}
      </p>
      <div className="flex items-center gap-2 pt-0.5">
        <button type="button" className="btn btn-primary !py-1.5 text-[12px]" onClick={() => onAccept(polished)}>
          Use it
        </button>
        <button type="button" className="btn btn-ghost !py-1.5 text-[12px]" onClick={onKeep}>
          Keep mine
        </button>
      </div>
    </section>
  );
}

/** Standalone button + before/after card for reuse (e.g. Ask box). */
export function PromptPolish({
  text,
  repo,
  onAccept,
  disabled = false,
}: {
  text: string;
  repo: string | null;
  onAccept: (next: string) => void;
  disabled?: boolean;
}) {
  const { busy, error, draft, polish, clear } = usePromptPolish(text, repo);
  return (
    <div className="space-y-2">
      <PolishButton disabled={disabled || !text.trim()} busy={busy} onClick={polish} />
      {error && <p className="text-[11.5px] text-bad">{error}</p>}
      {draft && (
        <PolishDiffCard
          original={draft.original}
          polished={draft.polished}
          onAccept={(next) => {
            onAccept(next);
            clear();
          }}
          onKeep={clear}
        />
      )}
    </div>
  );
}

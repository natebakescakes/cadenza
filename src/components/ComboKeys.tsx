/** Render one key-combination as small mono kbd boxes (e.g. "p + t").
 *  Supports compound chords separated by " → " (e.g. "e + l → a + k"):
 *  each part renders as its own group of kbd boxes with an arrow between groups.
 */
export function ComboKeys({ combo }: { combo: string }) {
  const parts = combo.split(" → ").map((part) =>
    part.split("+").map((k) => k.trim()).filter(Boolean)
  );

  return (
    <span className="inline-flex flex-wrap items-center gap-1">
      {parts.map((keys, partIdx) => (
        <span key={partIdx} className="inline-flex items-center gap-1">
          {partIdx > 0 && (
            <span className="text-[11px] text-muted-foreground/40 px-0.5">→</span>
          )}
          {keys.map((key, keyIdx) => (
            <span key={`${key}-${keyIdx}`} className="inline-flex items-center gap-1">
              {keyIdx > 0 && (
                <span className="text-[10px] text-muted-foreground/50">+</span>
              )}
              <kbd className="inline-flex min-w-[1.1rem] items-center justify-center rounded border border-border bg-secondary/60 px-1 py-px font-mono text-[10px] leading-none text-foreground/80">
                {key}
              </kbd>
            </span>
          ))}
        </span>
      ))}
    </span>
  );
}

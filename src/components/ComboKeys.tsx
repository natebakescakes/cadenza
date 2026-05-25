/** Render one key-combination as small mono kbd boxes (e.g. "p + t"). */
export function ComboKeys({ combo }: { combo: string }) {
  const keys = combo.split("+").map((k) => k.trim()).filter(Boolean);
  return (
    <span className="inline-flex flex-wrap items-center gap-1">
      {keys.map((key, i) => (
        <span key={`${key}-${i}`} className="inline-flex items-center gap-1">
          {i > 0 && (
            <span className="text-[10px] text-muted-foreground/50">+</span>
          )}
          <kbd className="inline-flex min-w-[1.1rem] items-center justify-center rounded border border-border bg-secondary/60 px-1 py-px font-mono text-[10px] leading-none text-foreground/80">
            {key}
          </kbd>
        </span>
      ))}
    </span>
  );
}

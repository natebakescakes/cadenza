import { motion, AnimatePresence } from "framer-motion";
import { Keyboard, Waves, Zap } from "lucide-react";
import { EmptyState } from "@/components/EmptyState";
import { Card, CardContent, CardHeader } from "@/components/ui/card";
import { Badge } from "@/components/ui/badge";
import type { LiveBlock } from "@/hooks/useLiveSession";
import { formatWpm } from "@/lib/format";
import { cn } from "@/lib/utils";

const BLOCK_MS = 5 * 60 * 1000;

function blockLabel(blockStart: number): string {
  const start = new Date(blockStart);
  const end = new Date(blockStart + BLOCK_MS);
  const fmt = (d: Date) =>
    d.toLocaleTimeString("en-US", { hour: "numeric", minute: "2-digit" });
  return `${fmt(start)} – ${fmt(end)}`;
}

interface FoldedToken {
  text: string;
  source: "manual" | "chorded" | "arpeggio";
  count: number;
}

/**
 * Collapse runs of adjacent identical tokens (same text + source) into a
 * single entry carrying a count. Order is preserved; distinct tokens never
 * merge, so only consecutive repeats fold (e.g. "the the the" → the ×3).
 */
function foldRuns(words: string[], source: "manual" | "chorded" | "arpeggio"): FoldedToken[] {
  const out: FoldedToken[] = [];
  for (const text of words) {
    const last = out[out.length - 1];
    if (last && last.text === text && last.source === source) {
      last.count += 1;
    } else {
      out.push({ text, source, count: 1 });
    }
  }
  return out;
}

function WordChip({
  text,
  source,
  count,
}: {
  text: string;
  source: "manual" | "chorded" | "arpeggio";
  count: number;
}) {
  return (
    <motion.span
      layout
      initial={{ opacity: 0, scale: 0.88 }}
      animate={{ opacity: 1, scale: 1 }}
      transition={{ duration: 0.2, ease: [0.16, 1, 0.3, 1] }}
      className={cn(
        "inline-flex items-center rounded-md border px-2 py-0.5 font-mono text-xs font-medium",
        source === "chorded"
          ? "border-info/30 bg-info/10 text-info"
          : source === "arpeggio"
            ? "border-warning/30 bg-warning/10 text-warning"
            : "border-border bg-secondary/60 text-foreground/80",
      )}
    >
      {text}
      {source === "chorded" && (
        <Zap className="ml-1 size-2.5 shrink-0 opacity-70" />
      )}
      {source === "arpeggio" && (
        <Waves className="ml-1 size-2.5 shrink-0 opacity-70" />
      )}
      {count > 1 && (
        <span
          className={cn(
            "tnum ml-1 shrink-0 text-[10px] font-semibold tabular-nums",
            source === "chorded"
              ? "text-info/70"
              : source === "arpeggio"
                ? "text-warning/70"
                : "text-muted-foreground",
          )}
        >
          ×{count}
        </span>
      )}
    </motion.span>
  );
}

export function BlockCard({
  block,
  isLatest,
  bare = false,
}: {
  block: LiveBlock;
  isLatest: boolean;
  /** When true, render without the inner Card chrome (for single-block panels). */
  bare?: boolean;
}) {
  const allManual = [
    ...block.manualWords,
    ...block.liveEntries.filter((e) => e.source === "manual").map((e) => e.text),
  ];
  const allChorded = [
    ...block.chorded_words,
    ...block.liveEntries.filter((e) => e.source === "chorded").map((e) => e.text),
  ];
  const allArpeggio = [
    ...block.arpeggio_words,
    ...block.liveEntries.filter((e) => e.source === "arpeggio").map((e) => e.text),
  ];
  const totalWords = allManual.length + allChorded.length + allArpeggio.length;
  const foldedManual = foldRuns(allManual, "manual");
  const foldedChorded = foldRuns(allChorded, "chorded");
  const foldedArpeggio = foldRuns(allArpeggio, "arpeggio");

  const headerRow = (
    <div className="flex items-center justify-between gap-2">
      <div className="flex items-center gap-2">
        {isLatest && (
          <span className="size-1.5 rounded-full bg-success animate-pulse-soft" />
        )}
        <span className="text-xs font-medium text-muted-foreground">
          {blockLabel(block.blockStart)}
        </span>
      </div>
      <div className="flex items-center gap-1.5">
        {allManual.length > 0 && (
          <Badge
            variant="outline"
            className="tnum gap-1 px-1.5 py-0 text-[10px] text-muted-foreground"
          >
            <Keyboard className="size-2.5" />
            {allManual.length}
          </Badge>
        )}
        {allChorded.length > 0 && (
          <Badge
            variant="outline"
            className="tnum gap-1 px-1.5 py-0 text-[10px] text-info"
          >
            <Zap className="size-2.5" />
            {allChorded.length}
          </Badge>
        )}
        {allArpeggio.length > 0 && (
          <Badge
            variant="outline"
            className="tnum gap-1 px-1.5 py-0 text-[10px] text-warning"
          >
            <Waves className="size-2.5" />
            {allArpeggio.length}
          </Badge>
        )}
        {block.wpm > 0 && (
          <Badge className="tnum bg-gold/15 text-gold border-gold/25 px-1.5 py-0 text-[10px] font-semibold">
            {formatWpm(block.wpm)} wpm
          </Badge>
        )}
      </div>
    </div>
  );

  const chips =
    totalWords === 0 ? (
      <p className="text-[11px] italic text-muted-foreground/50">No words yet.</p>
    ) : (
      <div className="flex flex-wrap gap-1">
        <AnimatePresence initial={false}>
          {foldedManual.map((t, i) => (
            <WordChip
              key={`m-${block.blockStart}-${i}-${t.text}`}
              text={t.text}
              source="manual"
              count={t.count}
            />
          ))}
          {foldedChorded.map((t, i) => (
            <WordChip
              key={`c-${block.blockStart}-${i}-${t.text}`}
              text={t.text}
              source="chorded"
              count={t.count}
            />
          ))}
          {foldedArpeggio.map((t, i) => (
            <WordChip
              key={`a-${block.blockStart}-${i}-${t.text}`}
              text={t.text}
              source="arpeggio"
              count={t.count}
            />
          ))}
        </AnimatePresence>
      </div>
    );

  // Bare: no nested Card — used by single-block panels (the dashboard's
  // "Latest activity" already provides the surrounding card).
  if (bare) {
    return (
      <div className="space-y-2">
        {headerRow}
        {chips}
      </div>
    );
  }

  return (
    <motion.div
      layout
      initial={{ opacity: 0, y: 10 }}
      animate={{ opacity: 1, y: 0 }}
      transition={{ duration: 0.3, ease: [0.16, 1, 0.3, 1] }}
    >
      <Card className={cn("gap-0", isLatest && "ring-1 ring-gold/25")}>
        <CardHeader className="pb-2">{headerRow}</CardHeader>
        <CardContent>{chips}</CardContent>
      </Card>
    </motion.div>
  );
}

/**
 * Renders the list of 5-minute activity blocks. Shared between the Dashboard
 * (capped to the most recent few) and Analytics (full history).
 */
export function ActivityFeed({
  blocks,
  emptyHint = "Start typing — words and chords will appear here grouped into 5-minute windows.",
  bare = false,
}: {
  blocks: LiveBlock[];
  emptyHint?: string;
  /** Render blocks without the inner Card chrome (for single-block panels). */
  bare?: boolean;
}) {
  const hasBlocks = blocks.some(
    (b) =>
      b.manualWords.length > 0 ||
      b.chorded_words.length > 0 ||
      b.arpeggio_words.length > 0 ||
      b.liveEntries.length > 0,
  );

  if (!hasBlocks) {
    return <EmptyState icon={Zap} title="No activity yet" hint={emptyHint} />;
  }

  return (
    <AnimatePresence initial={false}>
      {blocks.map((block, i) => (
        <BlockCard key={block.blockStart} block={block} isLatest={i === 0} bare={bare} />
      ))}
    </AnimatePresence>
  );
}

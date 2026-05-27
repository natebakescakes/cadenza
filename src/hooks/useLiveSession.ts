import { useEffect, useRef, useState } from "react";
import { getRecentBlocks } from "../lib/api";
import { onChordLogged, onWordLogged, onWpm } from "../lib/api";
import type { ActivityBlock } from "../lib/types";

const BLOCK_MS = 5 * 60 * 1000;
const MAX_LIVE_ENTRIES = 200;
// Cap the in-memory block Map. A new 5-min block is created for every window
// the app stays open; without a cap the Map grows for the whole session and
// every event re-sorts + re-renders an ever-larger set — the "slower the
// longer it runs" regression. 288 blocks = 24h, matching the DB snapshot range.
const MAX_BLOCKS = 288;

export interface LiveEntry {
  text: string;
  source: "manual" | "chorded" | "arpeggio";
  ts: number;
}

export interface LiveBlock {
  blockStart: number;
  wpm: number;
  /** Live entries accumulated this session (not in DB yet). */
  liveEntries: LiveEntry[];
  /** Historical words from the DB snapshot (loaded on mount). */
  manualWords: string[];
  chorded_words: string[];
  arpeggio_words: string[];
}

export interface LiveSession {
  /** Most recent rolling WPM from the backend event stream. */
  currentWpm: number | null;
  /** Blocks newest-first, merging DB history + live events. */
  blocks: LiveBlock[];
}

function blockKey(ts: number): number {
  return Math.floor(ts / BLOCK_MS) * BLOCK_MS;
}

function dbBlockToLive(b: ActivityBlock): LiveBlock {
  return {
    blockStart: b.t,
    wpm: b.wpm,
    liveEntries: [],
    manualWords: b.manual_words,
    chorded_words: b.chorded_words,
    arpeggio_words: b.arpeggio_words,
  };
}

export function useLiveSession(): LiveSession {
  const blocksRef = useRef<Map<number, LiveBlock>>(new Map());
  const currentWpmRef = useRef<number | null>(null);
  // Cached newest-first order. Re-sorted only when the SET of block keys
  // changes (insert/evict); in-place updates to a block's entries/wpm don't
  // change ordering, so they reuse this and skip the sort.
  const sortedRef = useRef<LiveBlock[]>([]);
  const [state, setState] = useState<LiveSession>({ currentWpm: null, blocks: [] });

  // Drop oldest blocks beyond MAX_BLOCKS so the Map stays bounded.
  const prune = () => {
    const size = blocksRef.current.size;
    if (size <= MAX_BLOCKS) return;
    const keys = [...blocksRef.current.keys()].sort((a, b) => a - b);
    for (let i = 0; i < size - MAX_BLOCKS; i++) {
      blocksRef.current.delete(keys[i]);
    }
  };

  // Push a new render. `structural` = the key set changed (a block was added),
  // so prune + re-sort; otherwise reuse the cached order (a fresh array slice is
  // still needed so React sees a new reference and re-renders).
  const commit = (currentWpm: number | null, structural: boolean) => {
    if (structural) {
      prune();
      sortedRef.current = [...blocksRef.current.values()].sort(
        (a, b) => b.blockStart - a.blockStart,
      );
    }
    setState({ currentWpm, blocks: sortedRef.current.slice() });
  };

  // Seed from DB history on mount, then subscribe to live events.
  useEffect(() => {
    let cancelled = false;
    const unlisteners: Array<() => void> = [];

    // 1. Load DB snapshot.
    getRecentBlocks()
      .then((dbBlocks) => {
        if (cancelled) return;
        for (const b of dbBlocks) {
          blocksRef.current.set(b.t, dbBlockToLive(b));
        }
        commit(null, true);
      })
      .catch(() => {});

    // 2. Live word events — add to the current block's liveEntries.
    onWordLogged((rec) => {
      const ts = rec.last_used || Date.now();
      const key = blockKey(ts);
      let block = blocksRef.current.get(key);
      const created = !block;
      const entry: LiveEntry = { text: rec.word, source: "manual", ts };
      if (!block) {
        block = {
          blockStart: key,
          wpm: 0,
          liveEntries: [],
          manualWords: [],
          chorded_words: [],
          arpeggio_words: [],
        };
        blocksRef.current.set(key, block);
      }
      block.liveEntries.push(entry);
      if (block.liveEntries.length > MAX_LIVE_ENTRIES) block.liveEntries.shift();
      commit(currentWpmRef.current, created);
    })
      .then((fn) => unlisteners.push(fn))
      .catch(() => {});

    onChordLogged((rec) => {
      const ts = rec.last_used || Date.now();
      const key = blockKey(ts);
      let block = blocksRef.current.get(key);
      const created = !block;
      const source = rec.kind === "arpeggio" ? "arpeggio" : "chorded";
      const entry: LiveEntry = { text: rec.phrase, source: source as LiveEntry["source"], ts };
      if (!block) {
        block = {
          blockStart: key,
          wpm: 0,
          liveEntries: [],
          manualWords: [],
          chorded_words: [],
          arpeggio_words: [],
        };
        blocksRef.current.set(key, block);
      }
      block.liveEntries.push(entry);
      if (block.liveEntries.length > MAX_LIVE_ENTRIES) block.liveEntries.shift();
      commit(currentWpmRef.current, created);
    })
      .then((fn) => unlisteners.push(fn))
      .catch(() => {});

    // 3. WPM events — update the block's wpm and currentWpm.
    onWpm((sample) => {
      if (sample.source !== "rolling") return;
      currentWpmRef.current = sample.wpm;
      const key = blockKey(Date.now());
      let block = blocksRef.current.get(key);
      const created = !block;
      if (!block) {
        block = {
          blockStart: key,
          wpm: sample.wpm,
          liveEntries: [],
          manualWords: [],
          chorded_words: [],
          arpeggio_words: [],
        };
        blocksRef.current.set(key, block);
      } else {
        block.wpm = sample.wpm;
      }
      commit(sample.wpm, created);
    })
      .then((fn) => unlisteners.push(fn))
      .catch(() => {});

    return () => {
      cancelled = true;
      for (const fn of unlisteners) fn();
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  return state;
}

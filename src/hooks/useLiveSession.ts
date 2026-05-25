import { useEffect, useRef, useState } from "react";
import { getRecentBlocks } from "../lib/api";
import { onChordLogged, onWordLogged, onWpm } from "../lib/api";
import type { ActivityBlock } from "../lib/types";

const BLOCK_MS = 5 * 60 * 1000;

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
  const [state, setState] = useState<LiveSession>({ currentWpm: null, blocks: [] });

  const rebuild = (currentWpm: number | null) => {
    const blocks = [...blocksRef.current.values()].sort(
      (a, b) => b.blockStart - a.blockStart,
    );
    setState({ currentWpm, blocks });
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
        rebuild(null);
      })
      .catch(() => {});

    // 2. Live word events — add to the current block's liveEntries.
    onWordLogged((rec) => {
      const ts = rec.last_used || Date.now();
      const key = blockKey(ts);
      const block = blocksRef.current.get(key);
      const entry: LiveEntry = { text: rec.word, source: "manual", ts };
      if (block) {
        // Avoid duplicating if the DB snapshot already contains this word.
        block.liveEntries.push(entry);
      } else {
        blocksRef.current.set(key, {
          blockStart: key,
          wpm: 0,
          liveEntries: [entry],
          manualWords: [],
          chorded_words: [],
          arpeggio_words: [],
        });
      }
      rebuild(state.currentWpm);
    })
      .then((fn) => unlisteners.push(fn))
      .catch(() => {});

    onChordLogged((rec) => {
      const ts = rec.last_used || Date.now();
      const key = blockKey(ts);
      const block = blocksRef.current.get(key);
      const source = rec.kind === "arpeggio" ? "arpeggio" : "chorded";
      const entry: LiveEntry = { text: rec.phrase, source: source as LiveEntry["source"], ts };
      if (block) {
        block.liveEntries.push(entry);
      } else {
        blocksRef.current.set(key, {
          blockStart: key,
          wpm: 0,
          liveEntries: [entry],
          manualWords: [],
          chorded_words: [],
          arpeggio_words: [],
        });
      }
      rebuild(state.currentWpm);
    })
      .then((fn) => unlisteners.push(fn))
      .catch(() => {});

    // 3. WPM events — update the block's wpm and currentWpm.
    onWpm((sample) => {
      if (sample.source !== "rolling") return;
      const key = blockKey(Date.now());
      const block = blocksRef.current.get(key);
      if (block) {
        block.wpm = sample.wpm;
      } else {
        blocksRef.current.set(key, {
          blockStart: key,
          wpm: sample.wpm,
          liveEntries: [],
          manualWords: [],
          chorded_words: [],
          arpeggio_words: [],
        });
      }
      const blocks = [...blocksRef.current.values()].sort(
        (a, b) => b.blockStart - a.blockStart,
      );
      setState({ currentWpm: sample.wpm, blocks });
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

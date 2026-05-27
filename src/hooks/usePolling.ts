import { useCallback, useEffect, useRef, useState } from "react";

export interface PollResult<T> {
  data: T;
  loading: boolean;
  error: boolean;
  refresh: () => void;
}

/**
 * Generic polling hook. Calls `fetcher` on mount + every `intervalMs`.
 * NEVER crashes: any throw resolves to `fallback` and sets `error`.
 */
export function usePolling<T>(
  fetcher: () => Promise<T>,
  fallback: T,
  intervalMs = 4000,
): PollResult<T> {
  const [data, setData] = useState<T>(fallback);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState(false);
  const fetcherRef = useRef(fetcher);
  fetcherRef.current = fetcher;
  const mounted = useRef(true);
  // Guard against overlapping calls: if a fetch is still in flight when the
  // next interval fires, skip it. Without this, a fetcher slower than the
  // interval piles up concurrent calls (blocking threads / pending promises)
  // that peg the CPU and leak memory the longer the app runs.
  const inFlight = useRef(false);

  const run = useCallback(async () => {
    if (inFlight.current) return;
    inFlight.current = true;
    try {
      const result = await fetcherRef.current();
      if (!mounted.current) return;
      setData(result);
      setError(false);
    } catch {
      if (!mounted.current) return;
      setError(true);
    } finally {
      inFlight.current = false;
      if (mounted.current) setLoading(false);
    }
  }, []);

  useEffect(() => {
    mounted.current = true;
    void run();
    if (intervalMs <= 0) return () => void (mounted.current = false);
    const id = setInterval(() => void run(), intervalMs);
    return () => {
      mounted.current = false;
      clearInterval(id);
    };
  }, [run, intervalMs]);

  return { data, loading, error, refresh: run };
}

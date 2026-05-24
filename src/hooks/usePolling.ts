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

  const run = useCallback(async () => {
    try {
      const result = await fetcherRef.current();
      if (!mounted.current) return;
      setData(result);
      setError(false);
    } catch {
      if (!mounted.current) return;
      setError(true);
    } finally {
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

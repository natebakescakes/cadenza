import { useCallback, useEffect, useState } from "react";
import {
  loggingStatus,
  onLoggingState,
  startLogging,
  stopLogging,
} from "@/lib/api";
import type { LoggingState } from "@/lib/types";

const DEFAULT: LoggingState = { logging: false, db_unlocked: false };

export function useLoggingState() {
  const [state, setState] = useState<LoggingState>(DEFAULT);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    let unlisten: (() => void) | undefined;
    loggingStatus()
      .then(setState)
      .catch(() => setState(DEFAULT));
    onLoggingState(setState)
      .then((fn) => {
        unlisten = fn;
      })
      .catch(() => {});
    return () => unlisten?.();
  }, []);

  const toggle = useCallback(async () => {
    setBusy(true);
    try {
      if (state.logging) {
        await stopLogging();
        setState((s) => ({ ...s, logging: false }));
      } else {
        await startLogging();
        setState((s) => ({ ...s, logging: true }));
      }
    } catch {
      // refresh truth from backend on failure
      try {
        setState(await loggingStatus());
      } catch {
        /* ignore */
      }
    } finally {
      setBusy(false);
    }
  }, [state.logging]);

  const setUnlocked = useCallback((unlocked: boolean) => {
    setState((s) => ({ ...s, db_unlocked: unlocked }));
  }, []);

  return { state, busy, toggle, setUnlocked };
}

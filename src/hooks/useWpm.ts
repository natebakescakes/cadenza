import { getWpmSummary, getWpmTrend } from "../lib/api";
import type { WpmSample, WpmSummary } from "../lib/types";
import { usePolling } from "./usePolling";

const EMPTY_SUMMARY: WpmSummary = {
  rolling: 0,
  session: 0,
  overall: 0,
  chorded: 0,
  manual: 0,
};

export function useWpmSummary() {
  return usePolling<WpmSummary>(getWpmSummary, EMPTY_SUMMARY, 3000);
}

export type WpmRange = "day" | "week" | "month" | "live";

export function useWpmTrend(range: WpmRange) {
  // "live" has no backend trend query — caller handles it separately.
  return usePolling<WpmSample[]>(
    () => (range === "live" ? Promise.resolve([]) : getWpmTrend(range)),
    [],
    8000,
  );
}

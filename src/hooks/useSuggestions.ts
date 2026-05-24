import { getSuggestions } from "../lib/api";
import type { Suggestion } from "../lib/types";
import { usePolling } from "./usePolling";

export function useSuggestions(limit = 50) {
  return usePolling<Suggestion[]>(() => getSuggestions(limit), [], 6000);
}

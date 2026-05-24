import { listWords } from "../lib/api";
import type { WordRecord } from "../lib/types";
import { usePolling } from "./usePolling";

export function useWords(limit = 500, sortBy = "score", search = "") {
  return usePolling<WordRecord[]>(
    () => listWords(limit, sortBy, search),
    [],
    6000,
  );
}

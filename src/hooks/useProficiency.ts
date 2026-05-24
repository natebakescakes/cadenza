import { getProficiency } from "../lib/api";
import type { Proficiency } from "../lib/types";
import { usePolling } from "./usePolling";

export function useProficiency() {
  return usePolling<Proficiency[]>(getProficiency, [], 6000);
}

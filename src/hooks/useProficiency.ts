import { getProficiency } from "../lib/api";
import type { Proficiency } from "../lib/types";
import { usePolling } from "./usePolling";

export function useProficiency() {
  const { data, loading } = usePolling<Proficiency[]>(getProficiency, [], 6000);
  return { data, loading };
}

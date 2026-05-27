import { practiceDueCount } from "../lib/api";
import { usePolling } from "./usePolling";

/** Count of practice cards currently due. Polls so the dashboard widget stays fresh. */
export function usePracticeDueCount() {
  return usePolling<number>(practiceDueCount, 0, 30000);
}

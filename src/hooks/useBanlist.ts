import { useCallback, useEffect, useState } from "react";
import { banWord, listBanlist, unbanWord } from "@/lib/api";
import type { BanlistEntry } from "@/lib/types";

export function useBanlist() {
  const [entries, setEntries] = useState<BanlistEntry[]>([]);
  const [loading, setLoading] = useState(true);

  const refresh = useCallback(async () => {
    try {
      setEntries(await listBanlist());
    } catch {
      setEntries([]);
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  const ban = useCallback(
    async (word: string) => {
      const w = word.trim().toLowerCase();
      if (!w) return;
      try {
        await banWord(w);
      } catch {
        /* ignore */
      }
      await refresh();
    },
    [refresh],
  );

  const unban = useCallback(
    async (word: string) => {
      try {
        await unbanWord(word);
      } catch {
        /* ignore */
      }
      await refresh();
    },
    [refresh],
  );

  return { entries, loading, ban, unban, refresh };
}

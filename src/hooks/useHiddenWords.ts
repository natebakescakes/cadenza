import { useCallback, useEffect, useState } from "react";
import { hideWord, listHidden, unhideWord } from "@/lib/api";

export function useHiddenWords() {
  const [hidden, setHidden] = useState<string[]>([]);
  const [loading, setLoading] = useState(true);

  const refresh = useCallback(async () => {
    try {
      setHidden(await listHidden());
    } catch {
      setHidden([]);
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  const hide = useCallback(
    async (word: string) => {
      const w = word.trim().toLowerCase();
      if (!w) return;
      try {
        await hideWord(w);
      } catch {
        /* ignore */
      }
      await refresh();
    },
    [refresh],
  );

  const unhide = useCallback(
    async (word: string) => {
      try {
        await unhideWord(word);
      } catch {
        /* ignore */
      }
      await refresh();
    },
    [refresh],
  );

  return { hidden, loading, hide, unhide, refresh };
}

import { useCallback, useEffect, useState } from "react";
import { getSettings, setSettings as apiSetSettings } from "@/lib/api";
import type { Settings } from "@/lib/types";

const DEFAULT: Settings = {
  new_word_threshold_s: 2,
  chord_char_threshold_ms: 50,
  allowed_chars: "abcdefghijklmnopqrstuvwxyz",
  arpeggio_threshold_ms: 40,
  thresholds_auto: true,
};

export function useSettings() {
  const [settings, setSettings] = useState<Settings>(DEFAULT);
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);

  useEffect(() => {
    let active = true;
    getSettings()
      .then((s) => active && setSettings(s))
      .catch(() => active && setSettings(DEFAULT))
      .finally(() => active && setLoading(false));
    return () => {
      active = false;
    };
  }, []);

  const save = useCallback(async (next: Settings) => {
    setSaving(true);
    setSettings(next); // optimistic
    try {
      await apiSetSettings(next);
      return true;
    } catch {
      return false;
    } finally {
      setSaving(false);
    }
  }, []);

  return { settings, loading, saving, save };
}

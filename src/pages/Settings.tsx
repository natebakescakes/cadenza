import { useEffect, useState } from "react";
import { motion } from "framer-motion";
import { toast } from "sonner";
import { PageHeader } from "@/components/PageHeader";
import { Button } from "@/components/ui/button";
import { useSettings } from "@/hooks/useSettings";
import { useBanlist } from "@/hooks/useBanlist";
import { useHiddenWords } from "@/hooks/useHiddenWords";
import { useLoggingState } from "@/hooks/useLoggingState";
import { resyncDeviceThresholds } from "@/lib/api";
import type { Settings as SettingsType } from "@/lib/types";
import { DetectionCard } from "./settings/DetectionCard";
import { CoachingCard } from "./settings/CoachingCard";
import { PermissionsCard } from "./settings/PermissionsCard";
import { PrivacyCard } from "./settings/PrivacyCard";
import { HiddenWordsCard } from "./settings/HiddenWordsCard";

export default function Settings() {
  const { settings, loading, saving, save } = useSettings();
  const { entries, ban, unban } = useBanlist();
  const { hidden, unhide } = useHiddenWords();
  const { state, toggle, busy } = useLoggingState();
  const [draft, setDraft] = useState<SettingsType>(settings);
  const [newBan, setNewBan] = useState("");

  useEffect(() => {
    if (!loading) setDraft(settings);
  }, [loading, settings]);

  const dirty =
    draft.new_word_threshold_s !== settings.new_word_threshold_s ||
    draft.chord_char_threshold_ms !== settings.chord_char_threshold_ms ||
    draft.arpeggio_threshold_ms !== settings.arpeggio_threshold_ms ||
    draft.chord_confusion_window_ms !== settings.chord_confusion_window_ms ||
    draft.allowed_chars !== settings.allowed_chars ||
    draft.coaching_enabled !== settings.coaching_enabled ||
    draft.coaching_show_ms !== settings.coaching_show_ms ||
    draft.coaching_fade_ms !== settings.coaching_fade_ms ||
    draft.coaching_suggest_min_count !== settings.coaching_suggest_min_count ||
    draft.coaching_suggest_min_len !== settings.coaching_suggest_min_len ||
    draft.coaching_resurface_rate !== settings.coaching_resurface_rate ||
    draft.coaching_persist !== settings.coaching_persist ||
    draft.coaching_hide_mastered !== settings.coaching_hide_mastered;

  const handleSave = async () => {
    const ok = await save(draft);
    toast[ok ? "success" : "error"](ok ? "Settings saved." : "Couldn't save settings.");
  };

  const handleResync = async () => {
    try {
      await resyncDeviceThresholds();
      // Re-fetch settings so inputs reflect the newly derived values.
      const ok = await save({ ...draft, thresholds_auto: true });
      toast[ok ? "success" : "error"](ok ? "Thresholds resynced from device." : "Resync failed.");
    } catch (e) {
      toast.error(`Resync failed: ${String(e)}`);
    }
  };

  return (
    <div>
      <PageHeader
        title="Settings"
        subtitle="Tune detection, manage privacy, and control logging."
        actions={
          dirty ? (
            <Button onClick={() => void handleSave()} disabled={saving}>
              Save changes
            </Button>
          ) : undefined
        }
      />

      <div className="space-y-4">
        {/* Detection thresholds */}
        <motion.div initial={{ opacity: 0, y: 8 }} animate={{ opacity: 1, y: 0 }} transition={{ duration: 0.35 }}>
          <DetectionCard
            draft={draft}
            setDraft={setDraft}
            thresholdsAuto={settings.thresholds_auto}
            onResync={() => void handleResync()}
          />
        </motion.div>

        {/* Coaching overlay */}
        <motion.div initial={{ opacity: 0, y: 8 }} animate={{ opacity: 1, y: 0 }} transition={{ duration: 0.35, delay: 0.04 }}>
          <CoachingCard draft={draft} setDraft={setDraft} />
        </motion.div>

        {/* macOS permissions */}
        <motion.div initial={{ opacity: 0, y: 8 }} animate={{ opacity: 1, y: 0 }} transition={{ duration: 0.35, delay: 0.08 }}>
          <PermissionsCard />
        </motion.div>

        {/* Privacy: logging + banlist */}
        <motion.div initial={{ opacity: 0, y: 8 }} animate={{ opacity: 1, y: 0 }} transition={{ duration: 0.35, delay: 0.12 }}>
          <PrivacyCard
            loggingOn={state.logging}
            loggingBusy={busy}
            onToggleLogging={toggle}
            banEntries={entries}
            onBan={ban}
            onUnban={unban}
            newBan={newBan}
            setNewBan={setNewBan}
          />
        </motion.div>

        {/* Hidden words */}
        <motion.div initial={{ opacity: 0, y: 8 }} animate={{ opacity: 1, y: 0 }} transition={{ duration: 0.35, delay: 0.16 }}>
          <HiddenWordsCard hidden={hidden} onUnhide={unhide} />
        </motion.div>
      </div>
    </div>
  );
}

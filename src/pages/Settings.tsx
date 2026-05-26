import { useEffect, useState } from "react";
import { motion } from "framer-motion";
import {
  Accessibility,
  ExternalLink,
  Eye,
  EyeOff,
  GraduationCap,
  Pencil,
  Plus,
  ShieldOff,
  SlidersHorizontal,
  Sparkles,
  Trash2,
  X,
} from "lucide-react";
import { openUrl } from "@tauri-apps/plugin-opener";
import { toast } from "sonner";
import { PageHeader } from "@/components/PageHeader";
import { EmptyState } from "@/components/EmptyState";
import {
  Card,
  CardContent,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Switch } from "@/components/ui/switch";
import { Separator } from "@/components/ui/separator";
import {
  Tooltip,
  TooltipContent,
  TooltipProvider,
  TooltipTrigger,
} from "@/components/ui/tooltip";
import { useSettings } from "@/hooks/useSettings";
import { useBanlist } from "@/hooks/useBanlist";
import { useHiddenWords } from "@/hooks/useHiddenWords";
import { useLoggingState } from "@/hooks/useLoggingState";
import { resyncDeviceThresholds } from "@/lib/api";
import type { Settings as SettingsType } from "@/lib/types";

const ACCESSIBILITY_URL =
  "x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility";
const INPUT_MONITORING_URL =
  "x-apple.systempreferences:com.apple.preference.security?Privacy_ListenEvent";

function SettingRow({
  label,
  hint,
  children,
}: {
  label: string;
  hint?: string;
  children: React.ReactNode;
}) {
  return (
    <div className="flex items-center justify-between gap-6 py-3.5">
      <div className="min-w-0">
        <p className="text-sm font-medium text-foreground">{label}</p>
        {hint && <p className="mt-0.5 text-xs text-muted-foreground">{hint}</p>}
      </div>
      <div className="shrink-0">{children}</div>
    </div>
  );
}

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

  const openSetting = async (url: string, fallback: string) => {
    try {
      await openUrl(url);
    } catch {
      toast.message("Open System Settings manually", { description: fallback });
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
          <Card>
            <CardHeader className="flex-row items-center justify-between">
              <CardTitle className="flex items-center gap-2">
                <SlidersHorizontal className="size-4 text-gold" /> Detection
              </CardTitle>
              <TooltipProvider>
                <Tooltip>
                  <TooltipTrigger asChild>
                    <Badge
                      className={
                        settings.thresholds_auto
                          ? "gap-1.5 bg-info/15 text-info border-info/25 cursor-default"
                          : "gap-1.5 bg-secondary text-muted-foreground border-border cursor-pointer"
                      }
                      onClick={settings.thresholds_auto ? undefined : () => void handleResync()}
                    >
                      {settings.thresholds_auto ? (
                        <><Sparkles className="size-3" /> Synced from device</>
                      ) : (
                        <><Pencil className="size-3" /> Manual — click to resync</>
                      )}
                    </Badge>
                  </TooltipTrigger>
                  <TooltipContent side="left" className="max-w-56">
                    {settings.thresholds_auto
                      ? "Thresholds are auto-derived from device settings on connect/refresh. Edit a value to switch to manual."
                      : "Thresholds are manually set. Click to re-derive from the connected device."}
                  </TooltipContent>
                </Tooltip>
              </TooltipProvider>
            </CardHeader>
            <CardContent className="divide-y divide-border">
              <SettingRow
                label="New word threshold"
                hint="Pause (seconds) that marks the end of a word."
              >
                <div className="flex items-center gap-2">
                  <Input
                    type="number"
                    step="0.1"
                    min="0"
                    value={draft.new_word_threshold_s}
                    onChange={(e) =>
                      setDraft({ ...draft, new_word_threshold_s: Number(e.target.value) })
                    }
                    className="w-24 tabular-nums"
                  />
                  <span className="text-xs text-muted-foreground">s</span>
                </div>
              </SettingRow>
              <SettingRow
                label="Chord character threshold"
                hint="Max ms between keys to count as a chord, not manual typing."
              >
                <div className="flex items-center gap-2">
                  <Input
                    type="number"
                    step="1"
                    min="0"
                    value={draft.chord_char_threshold_ms}
                    onChange={(e) =>
                      setDraft({ ...draft, chord_char_threshold_ms: Number(e.target.value) })
                    }
                    className="w-24 tabular-nums"
                  />
                  <span className="text-xs text-muted-foreground">ms</span>
                </div>
              </SettingRow>
              <SettingRow
                label="Arpeggio threshold"
                hint="Max ms between any two keys for a known chord phrase typed via arpeggio or compound chord to still count as chorded."
              >
                <div className="flex items-center gap-2">
                  <Input
                    type="number"
                    step="1"
                    min="0"
                    value={draft.arpeggio_threshold_ms}
                    onChange={(e) =>
                      setDraft({ ...draft, arpeggio_threshold_ms: Number(e.target.value) })
                    }
                    className="w-24 tabular-nums"
                  />
                  <span className="text-xs text-muted-foreground">ms</span>
                </div>
              </SettingRow>
              <SettingRow
                label="Chord confusion window"
                hint="After deleting a chord output, firing a different chord within this window is logged as a confusion event."
              >
                <div className="flex items-center gap-2">
                  <Input
                    type="number"
                    step="100"
                    min="0"
                    value={draft.chord_confusion_window_ms}
                    onChange={(e) =>
                      setDraft({ ...draft, chord_confusion_window_ms: Number(e.target.value) })
                    }
                    className="w-24 tabular-nums"
                  />
                  <span className="text-xs text-muted-foreground">ms</span>
                </div>
              </SettingRow>
              <div className="py-3.5">
                <Label htmlFor="allowed" className="text-sm font-medium text-foreground">
                  Allowed characters
                </Label>
                <p className="mt-0.5 mb-2 text-xs text-muted-foreground">
                  Only these characters are recorded as part of words.
                </p>
                <Input
                  id="allowed"
                  value={draft.allowed_chars}
                  onChange={(e) => setDraft({ ...draft, allowed_chars: e.target.value })}
                  className="font-mono text-sm"
                  placeholder="abcdefghijklmnopqrstuvwxyz"
                />
              </div>
            </CardContent>
          </Card>
        </motion.div>

        {/* Coaching overlay */}
        <motion.div initial={{ opacity: 0, y: 8 }} animate={{ opacity: 1, y: 0 }} transition={{ duration: 0.35, delay: 0.04 }}>
          <Card>
            <CardHeader>
              <CardTitle className="flex items-center gap-2">
                <GraduationCap className="size-4 text-gold" /> Coaching overlay
              </CardTitle>
            </CardHeader>
            <CardContent className="divide-y divide-border">
              <SettingRow
                label="Show coaching overlay"
                hint="After you manually type a word that has a chord, briefly flash its key combo near the caret."
              >
                <Switch
                  checked={draft.coaching_enabled}
                  onCheckedChange={(v) =>
                    setDraft({ ...draft, coaching_enabled: v })
                  }
                  aria-label="Toggle coaching overlay"
                />
              </SettingRow>
              <SettingRow
                label="Visible duration"
                hint="How long (ms) the overlay stays fully visible before it fades out."
              >
                <div className="flex items-center gap-2">
                  <Input
                    type="number"
                    step="100"
                    min="0"
                    value={draft.coaching_show_ms}
                    onChange={(e) =>
                      setDraft({ ...draft, coaching_show_ms: Number(e.target.value) })
                    }
                    className="w-24 tabular-nums"
                  />
                  <span className="text-xs text-muted-foreground">ms</span>
                </div>
              </SettingRow>
              <SettingRow
                label="Fade duration"
                hint="How long (ms) the overlay takes to fade out once its visible time elapses."
              >
                <div className="flex items-center gap-2">
                  <Input
                    type="number"
                    step="50"
                    min="0"
                    value={draft.coaching_fade_ms}
                    onChange={(e) =>
                      setDraft({ ...draft, coaching_fade_ms: Number(e.target.value) })
                    }
                    className="w-24 tabular-nums"
                  />
                  <span className="text-xs text-muted-foreground">ms</span>
                </div>
              </SettingRow>
              <SettingRow
                label="Suggested-combo threshold"
                hint="A chordless word must be typed manually at least this many times before a suggested combo is shown."
              >
                <div className="flex items-center gap-2">
                  <Input
                    type="number"
                    step="1"
                    min="0"
                    value={draft.coaching_suggest_min_count}
                    onChange={(e) =>
                      setDraft({ ...draft, coaching_suggest_min_count: Number(e.target.value) })
                    }
                    className="w-24 tabular-nums"
                  />
                  <span className="text-xs text-muted-foreground">×</span>
                </div>
              </SettingRow>
              <SettingRow
                label="Suggested-combo min length"
                hint="Don't offer a suggested combo for words shorter than this. Filters out very short tokens (e.g. 2-letter mouseless grid labels) that barely benefit from a chord."
              >
                <div className="flex items-center gap-2">
                  <Input
                    type="number"
                    step="1"
                    min="2"
                    value={draft.coaching_suggest_min_len}
                    onChange={(e) =>
                      setDraft({ ...draft, coaching_suggest_min_len: Number(e.target.value) })
                    }
                    className="w-24 tabular-nums"
                  />
                  <span className="text-xs text-muted-foreground">chars</span>
                </div>
              </SettingRow>
              <SettingRow
                label="Resurface rate"
                hint="Usage rate below which a previously-mastered chord's reminder returns (0–1). Lower = the chord must regress more before reminding you again."
              >
                <div className="flex items-center gap-2">
                  <Input
                    type="number"
                    step="0.05"
                    min="0"
                    max="1"
                    value={draft.coaching_resurface_rate}
                    onChange={(e) =>
                      setDraft({ ...draft, coaching_resurface_rate: Number(e.target.value) })
                    }
                    className="w-24 tabular-nums"
                  />
                </div>
              </SettingRow>
              <SettingRow
                label="Keep overlay visible"
                hint="Overlay stays until the next word (for inspecting placement/options); no auto-dismiss."
              >
                <Switch
                  checked={draft.coaching_persist}
                  onCheckedChange={(v) =>
                    setDraft({ ...draft, coaching_persist: v })
                  }
                  aria-label="Toggle coaching persist mode"
                />
              </SettingRow>
              <SettingRow
                label="Hide mastered chords"
                hint="Off by default — show a reminder for every chord you type manually. Turn on to stop reminding you of chords you've already mastered."
              >
                <Switch
                  checked={draft.coaching_hide_mastered}
                  onCheckedChange={(v) =>
                    setDraft({ ...draft, coaching_hide_mastered: v })
                  }
                  aria-label="Toggle hide mastered chords"
                />
              </SettingRow>
            </CardContent>
          </Card>
        </motion.div>

        {/* macOS permissions */}
        <motion.div initial={{ opacity: 0, y: 8 }} animate={{ opacity: 1, y: 0 }} transition={{ duration: 0.35, delay: 0.08 }}>
          <Card>
            <CardHeader>
              <CardTitle className="flex items-center gap-2">
                <Accessibility className="size-4 text-gold" /> macOS permissions
              </CardTitle>
            </CardHeader>
            <CardContent>
              <p className="mb-4 text-sm text-muted-foreground">
                To log keystrokes globally, Cadenza needs two macOS privileges. You may need to
                restart the app after granting them.
              </p>
              <div className="grid grid-cols-1 gap-3 sm:grid-cols-2">
                <div className="rounded-xl border border-border bg-secondary/30 p-4">
                  <Accessibility className="mb-2 size-5 text-gold" />
                  <p className="text-sm font-medium text-foreground">Accessibility</p>
                  <p className="mt-0.5 mb-3 text-xs text-muted-foreground">
                    Lets Cadenza observe keyboard input system-wide.
                  </p>
                  <Button
                    size="sm"
                    variant="secondary"
                    className="w-full"
                    onClick={() =>
                      void openSetting(
                        ACCESSIBILITY_URL,
                        "System Settings › Privacy & Security › Accessibility",
                      )
                    }
                  >
                    Open settings <ExternalLink className="size-3.5" />
                  </Button>
                </div>
                <div className="rounded-xl border border-border bg-secondary/30 p-4">
                  <Eye className="mb-2 size-5 text-gold" />
                  <p className="text-sm font-medium text-foreground">Input Monitoring</p>
                  <p className="mt-0.5 mb-3 text-xs text-muted-foreground">
                    Required to read individual key events for WPM.
                  </p>
                  <Button
                    size="sm"
                    variant="secondary"
                    className="w-full"
                    onClick={() =>
                      void openSetting(
                        INPUT_MONITORING_URL,
                        "System Settings › Privacy & Security › Input Monitoring",
                      )
                    }
                  >
                    Open settings <ExternalLink className="size-3.5" />
                  </Button>
                </div>
              </div>
            </CardContent>
          </Card>
        </motion.div>

        {/* Privacy: logging + banlist */}
        <motion.div initial={{ opacity: 0, y: 8 }} animate={{ opacity: 1, y: 0 }} transition={{ duration: 0.35, delay: 0.12 }}>
          <Card>
            <CardHeader>
              <CardTitle className="flex items-center gap-2">
                <ShieldOff className="size-4 text-gold" /> Privacy
              </CardTitle>
            </CardHeader>
            <CardContent>
              <SettingRow
                label="Keystroke logging"
                hint={state.logging ? "Cadenza is currently recording your typing." : "Logging is paused — nothing is recorded."}
              >
                <Switch
                  checked={state.logging}
                  onCheckedChange={() => void toggle()}
                  disabled={busy}
                  aria-label="Toggle keystroke logging"
                />
              </SettingRow>

              <Separator className="my-2" />

              <div className="py-3">
                <p className="text-sm font-medium text-foreground">Banned words</p>
                <p className="mt-0.5 mb-3 text-xs text-muted-foreground">
                  Words on this list are never recorded or suggested. Use it for passwords and
                  sensitive terms.
                </p>
                <form
                  onSubmit={(e) => {
                    e.preventDefault();
                    if (newBan.trim()) {
                      void ban(newBan);
                      setNewBan("");
                    }
                  }}
                  className="mb-3 flex gap-2"
                >
                  <Input
                    value={newBan}
                    onChange={(e) => setNewBan(e.target.value)}
                    placeholder="Add a word to never log…"
                    className="flex-1 font-mono text-sm"
                  />
                  <Button type="submit" variant="secondary" disabled={!newBan.trim()}>
                    <Plus className="size-4" /> Add
                  </Button>
                </form>

                {entries.length ? (
                  <div className="flex flex-wrap gap-2">
                    {entries.map((e) => (
                      <span
                        key={e.word}
                        className="group inline-flex items-center gap-1.5 rounded-full border border-border bg-secondary/50 py-1 pr-1.5 pl-2.5 text-xs"
                      >
                        <span className="font-mono text-foreground">{e.word}</span>
                        <button
                          aria-label={`Unban ${e.word}`}
                          onClick={() => void unban(e.word)}
                          className="grid size-4 place-items-center rounded-full text-muted-foreground transition-colors hover:bg-destructive/20 hover:text-destructive"
                        >
                          <X className="size-3" />
                        </button>
                      </span>
                    ))}
                  </div>
                ) : (
                  <EmptyState
                    compact
                    icon={Trash2}
                    title="No banned words"
                    hint="Anything you add here stays private and uncounted."
                  />
                )}
              </div>
            </CardContent>
          </Card>
        </motion.div>

        {/* Hidden words */}
        <motion.div initial={{ opacity: 0, y: 8 }} animate={{ opacity: 1, y: 0 }} transition={{ duration: 0.35, delay: 0.16 }}>
          <Card>
            <CardHeader>
              <CardTitle className="flex items-center gap-2">
                <EyeOff className="size-4 text-gold" /> Hidden words
              </CardTitle>
            </CardHeader>
            <CardContent>
              <p className="mb-4 text-sm text-muted-foreground">
                Hidden words are filtered out of the Words page and chord suggestions. Your typing
                data is preserved — unhide anytime to restore them.
              </p>
              {hidden.length ? (
                <div className="flex flex-wrap gap-2">
                  {hidden.map((w) => (
                    <span
                      key={w}
                      className="group inline-flex items-center gap-1.5 rounded-full border border-border bg-secondary/50 py-1 pr-1.5 pl-2.5 text-xs"
                    >
                      <span className="font-mono text-foreground">{w}</span>
                      <button
                        aria-label={`Unhide ${w}`}
                        onClick={() => void unhide(w)}
                        className="grid size-4 place-items-center rounded-full text-muted-foreground transition-colors hover:bg-info/20 hover:text-info"
                      >
                        <Eye className="size-3" />
                      </button>
                    </span>
                  ))}
                </div>
              ) : (
                <EmptyState
                  compact
                  icon={EyeOff}
                  title="No hidden words"
                  hint="Use the eye-off button on any word or suggestion to hide it here."
                />
              )}
            </CardContent>
          </Card>
        </motion.div>
      </div>
    </div>
  );
}

import { GraduationCap } from "lucide-react";
import {
  Card,
  CardContent,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Switch } from "@/components/ui/switch";
import type { Settings as SettingsType } from "@/lib/types";
import { SettingRow } from "./SettingRow";

interface CoachingCardProps {
  draft: SettingsType;
  setDraft: (s: SettingsType) => void;
}

export function CoachingCard({ draft, setDraft }: CoachingCardProps) {
  return (
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
  );
}

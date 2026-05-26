import {
  Pencil,
  SlidersHorizontal,
  Sparkles,
} from "lucide-react";
import {
  Card,
  CardContent,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import { Badge } from "@/components/ui/badge";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import {
  Tooltip,
  TooltipContent,
  TooltipProvider,
  TooltipTrigger,
} from "@/components/ui/tooltip";
import type { Settings as SettingsType } from "@/lib/types";
import { SettingRow } from "./SettingRow";

interface DetectionCardProps {
  draft: SettingsType;
  setDraft: (s: SettingsType) => void;
  thresholdsAuto: boolean;
  onResync: () => void;
}

export function DetectionCard({
  draft,
  setDraft,
  thresholdsAuto,
  onResync,
}: DetectionCardProps) {
  return (
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
                  thresholdsAuto
                    ? "gap-1.5 bg-info/15 text-info border-info/25 cursor-default"
                    : "gap-1.5 bg-secondary text-muted-foreground border-border cursor-pointer"
                }
                onClick={thresholdsAuto ? undefined : () => void onResync()}
              >
                {thresholdsAuto ? (
                  <><Sparkles className="size-3" /> Synced from device</>
                ) : (
                  <><Pencil className="size-3" /> Manual — click to resync</>
                )}
              </Badge>
            </TooltipTrigger>
            <TooltipContent side="left" className="max-w-56">
              {thresholdsAuto
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
  );
}

import { Languages } from "lucide-react";
import {
  Card,
  CardContent,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import { cn } from "@/lib/utils";
import type { Settings as SettingsType } from "@/lib/types";
import { SettingRow } from "./SettingRow";

interface EnglishVariantCardProps {
  draft: SettingsType;
  setDraft: (s: SettingsType) => void;
}

const VARIANTS = [
  { value: "uk", label: "UK" },
  { value: "us", label: "US" },
] as const;

export function EnglishVariantCard({ draft, setDraft }: EnglishVariantCardProps) {
  // Empty/unset stored value falls back to "us" for the active highlight.
  const current = draft.english_variant === "uk" ? "uk" : "us";
  return (
    <Card>
      <CardHeader>
        <CardTitle className="flex items-center gap-2">
          <Languages className="size-4 text-gold" /> English variant
        </CardTitle>
      </CardHeader>
      <CardContent>
        <SettingRow
          label="Spelling"
          hint="Biases generated practice sentences toward your preferred spelling (e.g. colour vs color), matching the spelling your chords produce."
        >
          <div
            role="radiogroup"
            aria-label="English variant"
            className="inline-flex rounded-lg border border-border bg-secondary/40 p-0.5"
          >
            {VARIANTS.map((v) => (
              <button
                key={v.value}
                type="button"
                role="radio"
                aria-checked={current === v.value}
                onClick={() => setDraft({ ...draft, english_variant: v.value })}
                className={cn(
                  "rounded-md px-3 py-1 text-xs font-medium transition-colors",
                  current === v.value
                    ? "bg-background text-foreground shadow-sm"
                    : "text-muted-foreground/70 hover:text-foreground",
                )}
              >
                {v.label}
              </button>
            ))}
          </div>
        </SettingRow>
      </CardContent>
    </Card>
  );
}

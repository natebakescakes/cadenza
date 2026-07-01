import { useRef, useState } from "react";
import { Keyboard, RotateCcw } from "lucide-react";
import {
  Card,
  CardContent,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import { Button } from "@/components/ui/button";
import type { Settings as SettingsType } from "@/lib/types";
import { cn } from "@/lib/utils";
import { SettingRow } from "./SettingRow";

// Canonical defaults — must match useSettings.ts / the Rust `shortcuts` module.
const DEFAULT_RELOAD = "CmdOrCtrl+Shift+R";
const DEFAULT_FORCE_COACHING = "CmdOrCtrl+Shift+C";

/** Pretty, macOS-symbol rendering of a Tauri accelerator string. */
function accelToDisplay(accel: string): string {
  if (!accel.trim()) return "Off";
  return accel
    .split("+")
    .map((tok) => {
      switch (tok) {
        case "CmdOrCtrl":
        case "CommandOrControl":
        case "Cmd":
        case "Command":
        case "Super":
          return "⌘";
        case "Ctrl":
        case "Control":
          return "⌃";
        case "Alt":
        case "Option":
          return "⌥";
        case "Shift":
          return "⇧";
        default:
          return tok;
      }
    })
    .join(" ");
}

/** Map a KeyboardEvent.code to a Tauri accelerator key token, or null for a
 *  pure-modifier press (so the recorder keeps waiting for a real key). */
function codeToKey(code: string): string | null {
  if (/^Key[A-Z]$/.test(code)) return code.slice(3);
  if (/^Digit[0-9]$/.test(code)) return code.slice(5);
  if (/^F\d{1,2}$/.test(code)) return code;
  const map: Record<string, string> = {
    Space: "Space",
    Enter: "Enter",
    Tab: "Tab",
    Backspace: "Backspace",
    Delete: "Delete",
    Home: "Home",
    End: "End",
    PageUp: "PageUp",
    PageDown: "PageDown",
    ArrowUp: "Up",
    ArrowDown: "Down",
    ArrowLeft: "Left",
    ArrowRight: "Right",
    Minus: "-",
    Equal: "=",
    BracketLeft: "[",
    BracketRight: "]",
    Semicolon: ";",
    Quote: "'",
    Backquote: "`",
    Backslash: "\\",
    Comma: ",",
    Period: ".",
    Slash: "/",
  };
  return map[code] ?? null;
}

/** Click-to-record accelerator field. Captures the next modifier+key chord and
 *  emits a Tauri accelerator string (e.g. "CmdOrCtrl+Shift+C"). A global
 *  shortcut without a modifier would hijack a plain key everywhere, so at least
 *  one modifier is required. Esc cancels recording. */
function ShortcutRecorder({
  value,
  defaultValue,
  onChange,
}: {
  value: string;
  defaultValue: string;
  onChange: (accel: string) => void;
}) {
  const [recording, setRecording] = useState(false);
  const btnRef = useRef<HTMLButtonElement>(null);

  const handleKeyDown = (e: React.KeyboardEvent) => {
    if (!recording) return;
    e.preventDefault();
    e.stopPropagation();
    if (e.key === "Escape") {
      setRecording(false);
      return;
    }

    const mods: string[] = [];
    if (e.metaKey || e.ctrlKey) mods.push("CmdOrCtrl");
    if (e.altKey) mods.push("Alt");
    if (e.shiftKey) mods.push("Shift");

    const key = codeToKey(e.code);
    if (!key) return; // pure modifier press — keep waiting
    if (mods.length === 0) return; // require a modifier for a global shortcut

    onChange([...mods, key].join("+"));
    setRecording(false);
    btnRef.current?.blur();
  };

  const isDefault = value === defaultValue;

  return (
    <div className="flex items-center gap-2">
      <button
        ref={btnRef}
        type="button"
        onClick={() => setRecording(true)}
        onKeyDown={handleKeyDown}
        onBlur={() => setRecording(false)}
        aria-label="Record shortcut"
        className={cn(
          "inline-flex h-8 min-w-28 items-center justify-center rounded-md border px-3 font-mono text-sm tabular-nums transition-colors",
          recording
            ? "border-gold/60 bg-gold/10 text-foreground ring-1 ring-gold/40"
            : "border-border bg-secondary/40 text-foreground hover:bg-secondary/70",
        )}
      >
        {recording ? "Press keys…" : accelToDisplay(value)}
      </button>
      {!isDefault && (
        <Button
          type="button"
          variant="ghost"
          size="icon"
          aria-label="Reset to default"
          title={`Reset to ${accelToDisplay(defaultValue)}`}
          onClick={() => onChange(defaultValue)}
          className="size-8"
        >
          <RotateCcw className="size-3.5" />
        </Button>
      )}
    </div>
  );
}

interface ShortcutsCardProps {
  draft: SettingsType;
  setDraft: (s: SettingsType) => void;
}

export function ShortcutsCard({ draft, setDraft }: ShortcutsCardProps) {
  return (
    <Card>
      <CardHeader>
        <CardTitle className="flex items-center gap-2">
          <Keyboard className="size-4 text-gold" /> Global shortcuts
        </CardTitle>
      </CardHeader>
      <CardContent className="divide-y divide-border">
        <SettingRow
          label="Reload chord library"
          hint="Re-read the chord map from your connected device in the background. Works from any app."
        >
          <ShortcutRecorder
            value={draft.shortcut_reload_chords}
            defaultValue={DEFAULT_RELOAD}
            onChange={(accel) =>
              setDraft({ ...draft, shortcut_reload_chords: accel })
            }
          />
        </SettingRow>
        <SettingRow
          label="Toggle coaching overlay"
          hint="Show the most recent word's chord suggestion on demand (even with the automatic overlay off); press again to hide it. Works from any app."
        >
          <ShortcutRecorder
            value={draft.shortcut_force_coaching}
            defaultValue={DEFAULT_FORCE_COACHING}
            onChange={(accel) =>
              setDraft({ ...draft, shortcut_force_coaching: accel })
            }
          />
        </SettingRow>
      </CardContent>
    </Card>
  );
}

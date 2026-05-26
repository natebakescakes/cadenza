import { Plus, ShieldOff, Trash2, X } from "lucide-react";
import { EmptyState } from "@/components/EmptyState";
import {
  Card,
  CardContent,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Switch } from "@/components/ui/switch";
import { Separator } from "@/components/ui/separator";
import type { BanlistEntry } from "@/lib/types";
import { SettingRow } from "./SettingRow";

interface PrivacyCardProps {
  loggingOn: boolean;
  loggingBusy: boolean;
  onToggleLogging: () => void;
  banEntries: BanlistEntry[];
  onBan: (w: string) => void;
  onUnban: (w: string) => void;
  newBan: string;
  setNewBan: (s: string) => void;
}

export function PrivacyCard({
  loggingOn,
  loggingBusy,
  onToggleLogging,
  banEntries,
  onBan,
  onUnban,
  newBan,
  setNewBan,
}: PrivacyCardProps) {
  return (
    <Card>
      <CardHeader>
        <CardTitle className="flex items-center gap-2">
          <ShieldOff className="size-4 text-gold" /> Privacy
        </CardTitle>
      </CardHeader>
      <CardContent>
        <SettingRow
          label="Keystroke logging"
          hint={loggingOn ? "Cadenza is currently recording your typing." : "Logging is paused — nothing is recorded."}
        >
          <Switch
            checked={loggingOn}
            onCheckedChange={() => void onToggleLogging()}
            disabled={loggingBusy}
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
                void onBan(newBan);
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

          {banEntries.length ? (
            <div className="flex flex-wrap gap-2">
              {banEntries.map((e) => (
                <span
                  key={e.word}
                  className="group inline-flex items-center gap-1.5 rounded-full border border-border bg-secondary/50 py-1 pr-1.5 pl-2.5 text-xs"
                >
                  <span className="font-mono text-foreground">{e.word}</span>
                  <button
                    aria-label={`Unban ${e.word}`}
                    onClick={() => void onUnban(e.word)}
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
  );
}

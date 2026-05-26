import { Eye, EyeOff } from "lucide-react";
import { EmptyState } from "@/components/EmptyState";
import {
  Card,
  CardContent,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";

interface HiddenWordsCardProps {
  hidden: string[];
  onUnhide: (w: string) => void;
}

export function HiddenWordsCard({ hidden, onUnhide }: HiddenWordsCardProps) {
  return (
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
                  onClick={() => void onUnhide(w)}
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
  );
}

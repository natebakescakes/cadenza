import { Accessibility, Eye, ExternalLink } from "lucide-react";
import { openUrl } from "@tauri-apps/plugin-opener";
import { toast } from "sonner";
import {
  Card,
  CardContent,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import { Button } from "@/components/ui/button";

const ACCESSIBILITY_URL =
  "x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility";
const INPUT_MONITORING_URL =
  "x-apple.systempreferences:com.apple.preference.security?Privacy_ListenEvent";

async function openSetting(url: string, fallback: string) {
  try {
    await openUrl(url);
  } catch {
    toast.message("Open System Settings manually", { description: fallback });
  }
}

export function PermissionsCard() {
  return (
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
  );
}

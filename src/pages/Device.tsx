import { useCallback, useEffect, useState } from "react";
import { motion } from "framer-motion";
import {
  Bug,
  Cable,
  CheckCircle2,
  Cpu,
  Hash,
  Loader2,
  Plug,
  RefreshCw,
  RotateCw,
  Search,
  SlidersHorizontal,
  Sparkles,
  Pencil,
} from "lucide-react";
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
import { Input } from "@/components/ui/input";
import { Badge } from "@/components/ui/badge";
import { Separator } from "@/components/ui/separator";
import {
  Tooltip,
  TooltipContent,
  TooltipProvider,
  TooltipTrigger,
} from "@/components/ui/tooltip";
import { useDevice } from "@/hooks/useDevice";
import { useSettings } from "@/hooks/useSettings";
import { debugDumpChords, getDeviceSettings, resyncDeviceThresholds } from "@/lib/api";
import type { DebugChordDump, DeviceSettings } from "@/lib/types";
import { formatNumber } from "@/lib/format";

function SpecRow({ label, value }: { label: string; value: string }) {
  return (
    <div className="flex items-center justify-between py-2.5 text-sm">
      <span className="text-muted-foreground">{label}</span>
      <span className="font-medium text-foreground">{value || "—"}</span>
    </div>
  );
}

function DevSettingRow({
  label,
  value,
  hint,
}: {
  label: string;
  value: string;
  hint?: string;
}) {
  return (
    <TooltipProvider>
      <Tooltip>
        <TooltipTrigger asChild>
          <div className="flex items-center justify-between py-2 cursor-default">
            <span className="text-muted-foreground">{label}</span>
            <span className="tnum font-medium text-foreground">{value}</span>
          </div>
        </TooltipTrigger>
        {hint && <TooltipContent side="left">{hint}</TooltipContent>}
      </Tooltip>
    </TooltipProvider>
  );
}

export default function Device() {
  const { device, ports, scanning, connecting, error, scan, connect, refreshMap } = useDevice();
  const { settings, save } = useSettings();
  const [deviceSettings, setDeviceSettings] = useState<DeviceSettings | null>(null);

  // DEBUG (temporary): raw CML C1 chord dump for reverse-engineering compound chords.
  const [dumpSearch, setDumpSearch] = useState("");
  const [dumpRows, setDumpRows] = useState<DebugChordDump[] | null>(null);
  const [dumping, setDumping] = useState(false);
  const [dumpError, setDumpError] = useState<string | null>(null);

  const handleDump = useCallback(async () => {
    setDumping(true);
    setDumpError(null);
    try {
      const rows = await debugDumpChords(dumpSearch.trim());
      setDumpRows(rows);
    } catch (e) {
      setDumpRows(null);
      setDumpError(String(e));
    } finally {
      setDumping(false);
    }
  }, [dumpSearch]);

  // Scan once on mount; fetch cached device settings if any.
  useEffect(() => {
    void scan();
    void getDeviceSettings().then(setDeviceSettings).catch(() => {});
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const handleConnect = async (port: string) => {
    const info = await connect(port);
    if (info) {
      toast.success(`Connected to ${info.name || info.device}`);
      // Refresh cached device settings after connect (backend derives thresholds there).
      void getDeviceSettings().then(setDeviceSettings).catch(() => {});
    } else {
      toast.error("Could not connect to that port.");
    }
  };

  const handleRefresh = async () => {
    const count = await refreshMap();
    toast.success(`Chord map refreshed — ${formatNumber(count)} chords loaded.`);
    void getDeviceSettings().then(setDeviceSettings).catch(() => {});
  };

  const handleResync = useCallback(async () => {
    try {
      await resyncDeviceThresholds();
      void getDeviceSettings().then(setDeviceSettings).catch(() => {});
      toast.success("Thresholds resynced from device.");
      // Reload settings so the Settings page input values update.
      await save({ ...settings, thresholds_auto: true });
    } catch (e) {
      toast.error(String(e));
    }
  }, [settings, save]);

  return (
    <div>
      <PageHeader
        title="Device"
        subtitle="Connect your CharaChorder to unlock proficiency analytics."
        actions={
          <Button variant="secondary" onClick={() => void scan()} disabled={scanning}>
            {scanning ? <Loader2 className="size-4 animate-spin" /> : <Search className="size-4" />}
            Scan
          </Button>
        }
      />

      <div className="grid grid-cols-1 gap-4 lg:grid-cols-5">
        {/* Connected device / connect card */}
        <motion.div
          initial={{ opacity: 0, y: 10 }}
          animate={{ opacity: 1, y: 0 }}
          transition={{ duration: 0.4 }}
          className="lg:col-span-3 flex flex-col gap-4"
        >
          <Card className="h-full">
            <CardHeader className="flex-row items-center justify-between">
              <CardTitle className="flex items-center gap-2">
                <Cpu className="size-4 text-gold" /> Connected device
              </CardTitle>
              {device && (
                <Badge className="gap-1.5 bg-success/15 text-success">
                  <CheckCircle2 className="size-3.5" /> Online
                </Badge>
              )}
            </CardHeader>
            <CardContent>
              {device ? (
                <div>
                  <div className="flex items-center gap-4 pb-2">
                    <div className="grid size-14 place-items-center rounded-2xl bg-gold/12 text-gold">
                      <Cable className="size-7" />
                    </div>
                    <div className="min-w-0">
                      <p className="font-display text-xl font-semibold tracking-tight text-foreground">
                        {device.name || device.device}
                      </p>
                      <p className="text-sm text-muted-foreground">{device.company}</p>
                    </div>
                  </div>
                  <Separator className="my-2" />
                  <SpecRow label="Device" value={device.device} />
                  <SpecRow label="Chipset" value={device.chipset} />
                  <SpecRow label="Firmware" value={device.version ? `v${device.version}` : ""} />
                  <SpecRow label="Port" value={device.port} />
                  <div className="flex items-center justify-between rounded-xl border border-border bg-secondary/40 px-3.5 py-3 mt-2">
                    <span className="flex items-center gap-2 text-sm text-muted-foreground">
                      <Hash className="size-4 text-gold" /> Chords on device
                    </span>
                    <span className="tnum text-lg font-semibold text-foreground">
                      {formatNumber(device.chord_count)}
                    </span>
                  </div>
                  <Button variant="secondary" className="mt-4 w-full" onClick={() => void handleRefresh()}>
                    <RotateCw className="size-4" /> Refresh chord map
                  </Button>
                </div>
              ) : (
                <EmptyState
                  icon={Plug}
                  title="No device connected"
                  hint="Select a detected port to connect your keyboard, or hit Scan to look again."
                />
              )}
            </CardContent>
          </Card>

          {/* Device settings card — shown once device settings have been read */}
          {deviceSettings && (
            <Card>
              <CardHeader className="flex-row items-center justify-between pb-3">
                <CardTitle className="flex items-center gap-2">
                  <SlidersHorizontal className="size-4 text-gold" /> Device settings
                </CardTitle>
                <TooltipProvider>
                  <Tooltip>
                    <TooltipTrigger asChild>
                      <Badge
                        className={
                          settings.thresholds_auto
                            ? "gap-1.5 bg-info/15 text-info border-info/25 cursor-default"
                            : "gap-1.5 bg-secondary text-muted-foreground border-border cursor-default"
                        }
                      >
                        {settings.thresholds_auto ? (
                          <><Sparkles className="size-3" /> Synced from device</>
                        ) : (
                          <><Pencil className="size-3" /> Manual</>
                        )}
                      </Badge>
                    </TooltipTrigger>
                    <TooltipContent side="left" className="max-w-56">
                      {settings.thresholds_auto
                        ? "Detection thresholds are auto-derived from the values below. Edit them in Settings to switch to manual."
                        : "You've set custom detection thresholds. Click Resync to re-derive from device values."}
                    </TooltipContent>
                  </Tooltip>
                </TooltipProvider>
              </CardHeader>
              <CardContent>
                <div className="divide-y divide-border text-sm">
                  <DevSettingRow
                    label="Output delay"
                    value={deviceSettings.output_delay_us >= 0 ? `${deviceSettings.output_delay_us} µs` : "—"}
                    hint="Inter-char emission spacing within a chord burst (id 0x17)"
                  />
                  <DevSettingRow
                    label="Arpeggiate timeout"
                    value={deviceSettings.arpeggiate_timeout_ms >= 0 ? `${deviceSettings.arpeggiate_timeout_ms} ms` : "—"}
                    hint={`Arpeggiate modifier window (id 0x54) — ${deviceSettings.arpeggiate_enabled ? "enabled" : "disabled"}`}
                  />
                  <DevSettingRow
                    label="Press tolerance"
                    value={deviceSettings.chord_press_tolerance_ms >= 0 ? `${deviceSettings.chord_press_tolerance_ms} ms` : "—"}
                    hint="Chord press co-detection window (id 0x34)"
                  />
                  <DevSettingRow
                    label="Release tolerance"
                    value={deviceSettings.chord_release_tolerance_ms >= 0 ? `${deviceSettings.chord_release_tolerance_ms} ms` : "—"}
                    hint="Chord release co-detection window (id 0x35)"
                  />
                  <DevSettingRow
                    label="Auto-delete timeout"
                    value={deviceSettings.auto_delete_timeout_ms >= 0 ? `${deviceSettings.auto_delete_timeout_ms} ms` : "—"}
                    hint="Chord auto-delete window (id 0x33)"
                  />
                  <DevSettingRow
                    label="Chording"
                    value={deviceSettings.chording_enabled ? "Enabled" : "Disabled"}
                    hint="Master chording toggle (id 0x31)"
                  />
                  <DevSettingRow
                    label="Spurring"
                    value={deviceSettings.spurring_enabled ? "Enabled" : "Disabled"}
                    hint="Spurring feature toggle (id 0x41)"
                  />
                </div>
                {!settings.thresholds_auto && (
                  <Button
                    variant="secondary"
                    size="sm"
                    className="mt-4 w-full"
                    onClick={() => void handleResync()}
                  >
                    <Sparkles className="size-3.5" /> Resync thresholds from device
                  </Button>
                )}
                <p className="mt-3 text-[11px] leading-relaxed text-muted-foreground/60">
                  Derived: chord threshold ≈ {(Math.max((deviceSettings.output_delay_us / 1000) * 3, 2)).toFixed(1)} ms ·
                  arpeggio ≈ {deviceSettings.arpeggiate_enabled && deviceSettings.arpeggiate_timeout_ms > 0
                    ? `${deviceSettings.arpeggiate_timeout_ms} ms`
                    : "40 ms (default)"
                  }. Verify against <code className="font-mono">[FLUSH]</code> avg_ms/max_ms in cadenza.log.
                </p>
              </CardContent>
            </Card>
          )}
        </motion.div>

        {/* Detected ports */}
        <motion.div
          initial={{ opacity: 0, y: 10 }}
          animate={{ opacity: 1, y: 0 }}
          transition={{ duration: 0.4, delay: 0.06 }}
          className="lg:col-span-2"
        >
          <Card className="h-full">
            <CardHeader className="flex-row items-center justify-between">
              <CardTitle>Detected ports</CardTitle>
              <button
                aria-label="Rescan ports"
                onClick={() => void scan()}
                className="grid size-7 place-items-center rounded-md text-muted-foreground transition-colors hover:bg-secondary hover:text-foreground"
              >
                <RefreshCw className={scanning ? "size-3.5 animate-spin" : "size-3.5"} />
              </button>
            </CardHeader>
            <CardContent>
              {error && (
                <p className="mb-3 rounded-lg border border-destructive/30 bg-destructive/10 px-3 py-2 text-xs text-destructive">
                  {error}
                </p>
              )}
              {ports.length ? (
                <ul className="space-y-2">
                  {ports.map((p) => {
                    const isConnected = device?.port === p.port;
                    return (
                      <li
                        key={p.port}
                        className="flex items-center justify-between gap-2 rounded-lg border border-border bg-secondary/30 px-3 py-2.5"
                      >
                        <div className="min-w-0">
                          <p className="truncate text-sm font-medium text-foreground">{p.name || p.port}</p>
                          <p className="truncate font-mono text-[11px] text-muted-foreground">{p.port}</p>
                        </div>
                        <Button
                          size="sm"
                          variant={isConnected ? "ghost" : "secondary"}
                          disabled={isConnected || connecting === p.port}
                          onClick={() => void handleConnect(p.port)}
                        >
                          {connecting === p.port ? (
                            <Loader2 className="size-3.5 animate-spin" />
                          ) : isConnected ? (
                            <>
                              <CheckCircle2 className="size-3.5 text-success" /> Connected
                            </>
                          ) : (
                            "Connect"
                          )}
                        </Button>
                      </li>
                    );
                  })}
                </ul>
              ) : (
                <EmptyState
                  compact
                  icon={Search}
                  title="No ports detected"
                  hint="Plug in your device over USB, then rescan."
                />
              )}
            </CardContent>
          </Card>
        </motion.div>
      </div>

      {/* DEBUG (temporary): raw CML C1 chord dump — reverse-engineering compound chords. */}
      <Card className="mt-4 border-dashed border-border/60">
        <CardHeader className="pb-3">
          <CardTitle className="flex items-center gap-2 text-sm text-muted-foreground">
            <Bug className="size-3.5" /> Debug — raw chord dump
          </CardTitle>
        </CardHeader>
        <CardContent>
          <div className="flex items-center gap-2">
            <Input
              value={dumpSearch}
              onChange={(e) => setDumpSearch(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === "Enter") void handleDump();
              }}
              placeholder="filter by phrase, e.g. touchpoint"
              className="h-8 font-mono text-xs"
            />
            <Button
              size="sm"
              variant="secondary"
              disabled={dumping}
              onClick={() => void handleDump()}
            >
              {dumping ? <Loader2 className="size-3.5 animate-spin" /> : <Bug className="size-3.5" />}
              Dump
            </Button>
          </div>

          {dumpError && (
            <p className="mt-3 rounded-lg border border-destructive/30 bg-destructive/10 px-3 py-2 text-xs text-destructive">
              {dumpError === "no device connected" ? "Connect a device first." : dumpError}
            </p>
          )}

          {!dumpError && dumpRows !== null && (
            dumpRows.length === 0 ? (
              <p className="mt-3 text-xs text-muted-foreground">No matches.</p>
            ) : (
              <div className="mt-3 overflow-x-auto rounded-lg border border-border bg-secondary/20">
                <table className="w-full select-text font-mono text-[11px]">
                  <thead className="text-muted-foreground">
                    <tr className="border-b border-border">
                      <th className="px-2 py-1.5 text-left font-medium">#</th>
                      <th className="px-2 py-1.5 text-left font-medium">phrase</th>
                      <th className="px-2 py-1.5 text-left font-medium">actionsHex</th>
                      <th className="px-2 py-1.5 text-left font-medium">phraseHex</th>
                    </tr>
                  </thead>
                  <tbody>
                    {dumpRows.map((r) => (
                      <tr key={r.index} className="border-b border-border/50 last:border-0">
                        <td className="px-2 py-1 text-muted-foreground tabular-nums">{r.index}</td>
                        <td className="px-2 py-1 whitespace-pre text-foreground">{r.phrase}</td>
                        <td className="px-2 py-1 break-all text-foreground">{r.actions_hex}</td>
                        <td className="px-2 py-1 break-all text-foreground">{r.phrase_hex}</td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              </div>
            )
          )}

          <p className="mt-3 text-[11px] leading-relaxed text-muted-foreground/60">
            Temporary tool: dumps the device&apos;s RAW <code className="font-mono">CML C1</code>{" "}
            response before any parsing (also logged to{" "}
            <code className="font-mono">cadenza.log</code> as <code className="font-mono">[DUMP]</code>).
            Select cells to copy. Leave the filter empty to dump every chord.
          </p>
        </CardContent>
      </Card>
    </div>
  );
}

import { useCallback, useEffect, useRef, useState } from "react";
import { Check, Download, Sparkles, Trash2 } from "lucide-react";
import { toast } from "sonner";
import {
  Card,
  CardContent,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  deleteModel,
  downloadModel,
  downloadRuntime,
  listModels,
  onModelDownloadProgress,
  runtimeReady,
  setActiveModel,
} from "@/lib/api";
import type { ModelEntry } from "@/lib/types";
import { cn } from "@/lib/utils";

/** Per-model in-flight download state (received/total bytes; null = idle). */
type Progress = { received: number; total: number };

function pct(p: Progress): number {
  if (p.total <= 0) return 0;
  return Math.min(100, Math.round((p.received / p.total) * 100));
}

function fmtMb(received: number): string {
  return `${(received / 1_048_576).toFixed(1)} MB`;
}

export function SentenceModelCard() {
  const [models, setModels] = useState<ModelEntry[] | null>(null);
  // Per-id download progress (present only while downloading that model). The
  // runtime download reuses this map under the synthetic id "runtime".
  const [progress, setProgress] = useState<Record<string, Progress>>({});
  // Per-id pending flag for activate/delete (disables the row's buttons).
  const [busy, setBusy] = useState<Record<string, boolean>>({});
  // Whether the one-time runtime (llama binary + dylibs) is installed.
  const [runtimeInstalled, setRuntimeInstalled] = useState<boolean | null>(null);

  const refresh = useCallback(() => {
    void listModels()
      .then(setModels)
      .catch(() => setModels([]));
    void runtimeReady()
      .then(setRuntimeInstalled)
      .catch(() => setRuntimeInstalled(false));
  }, []);

  useEffect(() => {
    refresh();
  }, [refresh]);

  // Single shared progress listener. Updates the in-flight map; on done/error it
  // clears that id and refreshes the catalog so badges/buttons settle.
  const refreshRef = useRef(refresh);
  refreshRef.current = refresh;
  useEffect(() => {
    let unlisten: (() => void) | undefined;
    void onModelDownloadProgress((e) => {
      if (e.error) {
        setProgress((p) => {
          const next = { ...p };
          delete next[e.id];
          return next;
        });
        toast.error(`Download failed: ${e.error}`);
        refreshRef.current();
        return;
      }
      if (e.done) {
        setProgress((p) => {
          const next = { ...p };
          delete next[e.id];
          return next;
        });
        if (e.id === "runtime") {
          setRuntimeInstalled(true);
          toast.success("Runtime installed.");
        } else {
          toast.success("Model downloaded.");
        }
        refreshRef.current();
        return;
      }
      setProgress((p) => ({ ...p, [e.id]: { received: e.received, total: e.total } }));
    }).then((fn) => {
      unlisten = fn;
    });
    return () => unlisten?.();
  }, []);

  // Any download currently in flight (disables other Download buttons so the
  // user can't start concurrent large downloads).
  const anyDownloading = Object.keys(progress).length > 0;

  const handleDownload = useCallback((id: string) => {
    setProgress((p) => ({ ...p, [id]: { received: 0, total: 0 } }));
    void downloadModel(id).catch((err: unknown) => {
      // The error event already surfaced a toast + cleared progress; this is the
      // promise-reject path. Clear here too in case the event was missed.
      setProgress((p) => {
        const next = { ...p };
        delete next[id];
        return next;
      });
      const msg = String(err ?? "");
      if (msg) toast.error(`Download failed: ${msg}`);
    });
  }, []);

  const handleDownloadRuntime = useCallback(() => {
    setProgress((p) => ({ ...p, runtime: { received: 0, total: 0 } }));
    void downloadRuntime().catch((err: unknown) => {
      // The error event already surfaced a toast + cleared progress; clear here
      // too in case the event was missed.
      setProgress((p) => {
        const next = { ...p };
        delete next.runtime;
        return next;
      });
      const msg = String(err ?? "");
      if (msg) toast.error(`Download failed: ${msg}`);
    });
  }, []);

  const handleActivate = useCallback(
    (id: string) => {
      setBusy((b) => ({ ...b, [id]: true }));
      void setActiveModel(id)
        .then(() => refreshRef.current())
        .catch((e: unknown) => toast.error(`Couldn't activate: ${String(e)}`))
        .finally(() => setBusy((b) => ({ ...b, [id]: false })));
    },
    [],
  );

  const handleDelete = useCallback(
    (id: string) => {
      setBusy((b) => ({ ...b, [id]: true }));
      void deleteModel(id)
        .then(() => refreshRef.current())
        .catch((e: unknown) => toast.error(`Couldn't delete: ${String(e)}`))
        .finally(() => setBusy((b) => ({ ...b, [id]: false })));
    },
    [],
  );

  return (
    <Card>
      <CardHeader>
        <CardTitle className="flex items-center gap-2">
          <Sparkles className="size-4 text-gold" /> Sentence model
        </CardTitle>
      </CardHeader>
      <CardContent>
        <p className="mb-4 text-sm text-muted-foreground">
          Sentence practice generates a line from a local model that runs on your
          machine. Download one to get started; pick the active model below.
        </p>

        {runtimeInstalled === false ? (
          (() => {
            const dl = progress.runtime;
            const downloading = dl != null;
            return (
              <div className="mb-4 rounded-xl border border-gold/40 bg-secondary/30 p-4">
                <div className="flex items-start justify-between gap-3">
                  <div className="min-w-0">
                    <p className="text-sm font-medium text-foreground">
                      Runtime required
                    </p>
                    <p className="mt-0.5 text-xs text-muted-foreground">
                      Sentence mode needs a one-time ~18 MB runtime download
                      before any model can run.
                    </p>
                  </div>
                  <Button
                    size="sm"
                    variant="secondary"
                    className="shrink-0"
                    disabled={downloading}
                    onClick={handleDownloadRuntime}
                  >
                    <Download className="size-3.5" />
                    {downloading ? "Downloading…" : "Download runtime"}
                  </Button>
                </div>
                {downloading && (
                  <div className="mt-3">
                    <div className="h-1.5 w-full overflow-hidden rounded-full bg-secondary">
                      <div
                        className="h-full rounded-full bg-gold transition-[width] duration-200"
                        style={{
                          width: dl.total > 0 ? `${pct(dl)}%` : "100%",
                        }}
                      />
                    </div>
                    <p className="tnum mt-1 text-[11px] text-muted-foreground/70">
                      {dl.total > 0
                        ? `${fmtMb(dl.received)} / ${fmtMb(dl.total)} · ${pct(dl)}%`
                        : `${fmtMb(dl.received)} downloaded…`}
                    </p>
                  </div>
                )}
              </div>
            );
          })()
        ) : runtimeInstalled === true ? (
          <p className="mb-4 flex items-center gap-1.5 text-xs text-muted-foreground">
            <Check className="size-3.5 text-gold" /> Runtime installed
          </p>
        ) : null}

        {models == null ? (
          <div className="space-y-2">
            {[0, 1, 2].map((i) => (
              <div
                key={i}
                className="h-20 animate-pulse rounded-xl border border-border bg-secondary/40"
              />
            ))}
          </div>
        ) : (
          <div className="space-y-3">
            {models.map((m) => {
              const dl = progress[m.id];
              const downloading = dl != null;
              const rowBusy = busy[m.id] === true;
              return (
                <div
                  key={m.id}
                  className={cn(
                    "rounded-xl border bg-secondary/30 p-4 transition-colors",
                    m.active ? "border-gold/40" : "border-border",
                  )}
                >
                  <div className="flex items-start justify-between gap-3">
                    <div className="min-w-0">
                      <div className="flex items-center gap-2">
                        <p className="text-sm font-medium text-foreground">
                          {m.name}
                        </p>
                        {m.active ? (
                          <Badge variant="outline" className="gap-1 text-gold">
                            <Check className="size-3" /> Active
                          </Badge>
                        ) : m.downloaded ? (
                          <Badge variant="outline" className="text-muted-foreground">
                            Downloaded
                          </Badge>
                        ) : null}
                      </div>
                      <p className="mt-0.5 text-xs text-muted-foreground">
                        {m.description} · ~{m.size_mb} MB
                      </p>
                    </div>

                    <div className="flex shrink-0 items-center gap-2">
                      {!m.downloaded && (
                        <Button
                          size="sm"
                          variant="secondary"
                          disabled={downloading || anyDownloading}
                          onClick={() => handleDownload(m.id)}
                        >
                          <Download className="size-3.5" />
                          {downloading ? "Downloading…" : "Download"}
                        </Button>
                      )}
                      {m.downloaded && !m.active && (
                        <Button
                          size="sm"
                          variant="secondary"
                          disabled={rowBusy}
                          onClick={() => handleActivate(m.id)}
                        >
                          Use
                        </Button>
                      )}
                      {m.downloaded && (
                        <Button
                          size="sm"
                          variant="ghost"
                          disabled={rowBusy || downloading}
                          onClick={() => handleDelete(m.id)}
                          aria-label={`Discard ${m.name}`}
                          title={
                            m.active
                              ? "Discard (will fall back to the default model)"
                              : "Discard"
                          }
                        >
                          <Trash2 className="size-3.5" />
                        </Button>
                      )}
                    </div>
                  </div>

                  {downloading && (
                    <div className="mt-3">
                      <div className="h-1.5 w-full overflow-hidden rounded-full bg-secondary">
                        <div
                          className="h-full rounded-full bg-gold transition-[width] duration-200"
                          style={{
                            width: dl.total > 0 ? `${pct(dl)}%` : "100%",
                          }}
                        />
                      </div>
                      <p className="tnum mt-1 text-[11px] text-muted-foreground/70">
                        {dl.total > 0
                          ? `${fmtMb(dl.received)} / ${fmtMb(dl.total)} · ${pct(dl)}%`
                          : `${fmtMb(dl.received)} downloaded…`}
                      </p>
                    </div>
                  )}
                </div>
              );
            })}
          </div>
        )}
      </CardContent>
    </Card>
  );
}

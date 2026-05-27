import { useEffect, useState, type ReactElement } from "react";
import { AnimatePresence } from "framer-motion";
import { onOverlayHide, onOverlayShow, onOverlayUpdate } from "@/lib/api";
import type { SyncSurfacePayload } from "@/lib/types";
import { SyncSurface } from "./SyncSurface";

// ── Surface registry ──────────────────────────────────────────────────────────
//
// Maps a surface `kind` -> the component that renders it. Each component takes
// the surface's `payload` plus an `onDone` callback to remove itself. To add a
// future surface: register one entry here (e.g. `menu: { component: MenuSurface }`).
interface SurfaceProps {
  payload: unknown;
  onDone: () => void;
}

const REGISTRY: Record<string, { component: (p: SurfaceProps) => ReactElement }> = {
  sync: {
    component: ({ payload, onDone }) => (
      <SyncSurface payload={payload as SyncSurfacePayload} onDone={onDone} />
    ),
  },
};

/**
 * SurfaceStack — the generic-surface "router". Subscribes to overlay:show /
 * overlay:update / overlay:hide, holds a Map<kind, payload> of active surfaces,
 * and renders the registered component for each. Stacks surfaces vertically
 * within the shared caret-anchored container. Coaching is NOT here — it has its
 * own component + dedicated events.
 */
export function SurfaceStack() {
  const [surfaces, setSurfaces] = useState<Map<string, unknown>>(new Map());

  useEffect(() => {
    const upsert = (kind: string, payload: unknown) => {
      if (!REGISTRY[kind]) return; // unknown kind — ignore
      setSurfaces((prev) => {
        const next = new Map(prev);
        next.set(kind, payload);
        return next;
      });
    };
    const remove = (kind: string) => {
      setSurfaces((prev) => {
        if (!prev.has(kind)) return prev;
        const next = new Map(prev);
        next.delete(kind);
        return next;
      });
    };

    const unShow = onOverlayShow((e) => upsert(e.kind, e.payload));
    const unUpdate = onOverlayUpdate((e) => upsert(e.kind, e.payload));
    const unHide = onOverlayHide((e) => remove(e.kind));

    return () => {
      void unShow.then((fn) => fn());
      void unUpdate.then((fn) => fn());
      void unHide.then((fn) => fn());
    };
  }, []);

  return (
    <div className="flex flex-col items-start gap-1.5">
      <AnimatePresence>
        {[...surfaces.entries()].map(([kind, payload]) => {
          const entry = REGISTRY[kind];
          if (!entry) return null;
          const Component = entry.component;
          return (
            <Component
              key={kind}
              payload={payload}
              onDone={() => {
                setSurfaces((prev) => {
                  if (!prev.has(kind)) return prev;
                  const next = new Map(prev);
                  next.delete(kind);
                  return next;
                });
              }}
            />
          );
        })}
      </AnimatePresence>
    </div>
  );
}

// Panel arbiter — ref-counts the single overlay NSPanel's shared lifecycle.
//
// The overlay is one NSPanel rendered by one React root, but it now hosts
// MULTIPLE coexisting surfaces (coaching, sync, future menus). Each surface
// wants to drive the panel's HIDE edge and its interactivity independently;
// done naively they collide (e.g. coaching's auto-hide would tear down the
// panel while a sync surface is still up). This module serialises that:
//
//   visibility   — every visible surface acquires; hideOverlay() fires ONLY on
//                  the last release (1 -> 0). Showing/positioning is owned by
//                  the backend (coaching engine + the hotkey path), so the
//                  arbiter never calls a show command — it owns only the hide.
//   interactivity — every surface wanting clickable controls acquires;
//                  setOverlayInteractive(true) on 0 -> 1, (false) on 1 -> 0.
//
// Module-level singleton (one React root). Tiny + synchronous; the underlying
// hideOverlay / setOverlayInteractive calls already swallow errors.

import { hideOverlay, setOverlayInteractive } from "./api";

let visibleCount = 0;
let interactiveCount = 0;

/** Mark a surface as wanting the panel shown. */
export function acquireVisibility(): void {
  visibleCount += 1;
}

/** Release a surface's visibility hold; hides the panel on the last release. */
export function releaseVisibility(): void {
  if (visibleCount === 0) return;
  visibleCount -= 1;
  if (visibleCount === 0) {
    void hideOverlay().catch(() => {});
  }
}

/** Mark a surface as wanting the panel interactive (clickable). */
export function acquireInteractive(): void {
  interactiveCount += 1;
  if (interactiveCount === 1) {
    void setOverlayInteractive(true);
  }
}

/** Release an interactivity hold; reverts to click-through on the last release. */
export function releaseInteractive(): void {
  if (interactiveCount === 0) return;
  interactiveCount -= 1;
  if (interactiveCount === 0) {
    void setOverlayInteractive(false);
  }
}

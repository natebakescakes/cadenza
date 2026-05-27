import { CoachingSurface } from "@/components/overlay/CoachingSurface";
import { SurfaceStack } from "@/components/overlay/SurfaceStack";

/**
 * Overlay root — the single caret-anchored, pointer-only NSPanel container that
 * hosts multiple coexisting surfaces. Runs in its own React root
 * (overlay-main.tsx) with NO providers (no router, DbGate, AppShell, Toaster,
 * or theme context beyond the `.dark` class). The "router" is just the
 * kind->component registry + Map state inside SurfaceStack.
 *
 * Anchor BOTTOM-left: the panel is positioned ABOVE the caret (Rust sets the
 * panel so its bottom edge sits just above the caret), so content hugs the
 * bottom of the panel. Fill the panel viewport (h-screen) and push content to
 * the bottom-left; the transparent area extends upward, invisible.
 *
 * Surfaces stack vertically within the anchor: coaching (its own lifecycle) and
 * the generic SurfaceStack (sync + future surfaces).
 */
export default function OverlayRoot() {
  return (
    <div className="flex h-screen w-screen flex-col items-start justify-end gap-1.5 bg-transparent p-1">
      <SurfaceStack />
      <CoachingSurface />
    </div>
  );
}

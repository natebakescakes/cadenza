import { useEffect } from "react";
import { check } from "@tauri-apps/plugin-updater";
import { relaunch } from "@tauri-apps/plugin-process";
import { toast } from "sonner";

export function UpdateChecker() {
  useEffect(() => {
    const run = async () => {
      try {
        const update = await check();
        if (!update) return;
        toast.info(`Update available: v${update.version}`, {
          description: update.body ?? "New version ready to install.",
          duration: Infinity,
          action: {
            label: "Install & Restart",
            onClick: async () => {
              await update.downloadAndInstall();
              await relaunch();
            },
          },
        });
      } catch {
        // no network, endpoint not live yet, etc.
      }
    };

    const t = setTimeout(run, 3000);
    return () => clearTimeout(t);
  }, []);

  return null;
}

import { relaunch } from "@tauri-apps/plugin-process";
import { check, type Update } from "@tauri-apps/plugin-updater";
import { Download } from "lucide-react";
import { useEffect, useState } from "react";
import { Button } from "@/components/ui/button";

/**
 * Offers the latest published release when it is newer than this GUI, checked once at start. Taking
 * it downloads the installer, checks its signature, installs it and restarts the GUI. A check that
 * fails, such as offline, offers nothing.
 */
export function UpdateButton({ onError }: { onError: (e: unknown) => void }) {
  const [update, setUpdate] = useState<Update | null>(null);
  const [installing, setInstalling] = useState(false);

  useEffect(() => {
    check().then(setUpdate, () => {});
  }, []);

  if (!update) return null;

  const install = async () => {
    setInstalling(true);
    try {
      // On Windows the installer closes the GUI and starts the new one itself.
      await update.downloadAndInstall();
      await relaunch();
    } catch (e) {
      setInstalling(false);
      onError(e);
    }
  };

  return (
    <Button variant="outline" size="sm" disabled={installing} onClick={install}>
      <Download /> {installing ? "Updating…" : `Update to ${update.version}`}
    </Button>
  );
}

// Choosing a Dev Server URL App Source: the bundler's address plus the
// App's config.xml. Shown inline under the toolbar rather than as a dialog,
// which the embedded Engine window would cover (ADR-0006).
import { open } from "@tauri-apps/plugin-dialog";
import { useState } from "react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";

export function DevServerForm({
  onOpen,
  onClose,
}: {
  onOpen: (url: string, config: string) => void;
  onClose: () => void;
}) {
  const [url, setUrl] = useState("http://localhost:5173/");
  const [config, setConfig] = useState("");

  const chooseConfig = async () => {
    const path = await open({ filters: [{ name: "config.xml", extensions: ["xml"] }] });
    if (typeof path === "string") setConfig(path);
  };

  return (
    <form
      aria-label="Run from a dev server"
      className="flex flex-wrap items-end gap-3 border-b px-3 py-2 text-sm"
      onSubmit={(e) => {
        e.preventDefault();
        onClose();
        onOpen(url.trim(), config);
      }}
    >
      <p className="basis-full text-muted-foreground">
        Your bundler serves the App; its config.xml gives the privileges and Access Policy.
      </p>
      <div className="grid gap-1">
        <Label htmlFor="dev-server-url">Dev Server URL</Label>
        <Input id="dev-server-url" value={url} onChange={(e) => setUrl(e.target.value)} required />
      </div>
      <div className="grid min-w-64 flex-1 gap-1">
        <Label htmlFor="dev-server-config">config.xml</Label>
        <div className="flex gap-2">
          <Input id="dev-server-config" value={config} onChange={(e) => setConfig(e.target.value)} required />
          <Button type="button" variant="outline" onClick={chooseConfig}>
            Browse…
          </Button>
        </div>
      </div>
      <Button type="submit">Run</Button>
      <Button type="button" variant="ghost" onClick={onClose}>
        Cancel
      </Button>
    </form>
  );
}

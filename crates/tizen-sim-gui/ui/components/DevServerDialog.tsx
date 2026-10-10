// Choosing a Dev Server URL App Source: the bundler's address plus the
// App's config.xml.
import { open } from "@tauri-apps/plugin-dialog";
import { Globe } from "lucide-react";
import { useState } from "react";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
  DialogTrigger,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";

export function DevServerDialog({ onOpen }: { onOpen: (url: string, config: string) => void }) {
  const [shown, setShown] = useState(false);
  const [url, setUrl] = useState("http://localhost:5173/");
  const [config, setConfig] = useState("");

  const chooseConfig = async () => {
    const path = await open({ filters: [{ name: "config.xml", extensions: ["xml"] }] });
    if (typeof path === "string") setConfig(path);
  };

  return (
    <Dialog open={shown} onOpenChange={setShown}>
      <DialogTrigger asChild>
        <Button variant="outline" size="sm">
          <Globe /> Dev server…
        </Button>
      </DialogTrigger>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>Run from a dev server</DialogTitle>
          <DialogDescription>
            Your bundler serves the App; its config.xml gives the privileges and Access Policy.
          </DialogDescription>
        </DialogHeader>
        <form
          className="grid gap-4"
          onSubmit={(e) => {
            e.preventDefault();
            setShown(false);
            onOpen(url.trim(), config);
          }}
        >
          <div className="grid gap-2">
            <Label htmlFor="dev-server-url">Dev Server URL</Label>
            <Input id="dev-server-url" value={url} onChange={(e) => setUrl(e.target.value)} required />
          </div>
          <div className="grid gap-2">
            <Label htmlFor="dev-server-config">config.xml</Label>
            <div className="flex gap-2">
              <Input id="dev-server-config" value={config} onChange={(e) => setConfig(e.target.value)} required />
              <Button type="button" variant="outline" onClick={chooseConfig}>
                Browse…
              </Button>
            </div>
          </div>
          <DialogFooter>
            <Button type="submit">Run</Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}

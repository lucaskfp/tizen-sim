// Sessions the CLI started, which the GUI can adopt. Shown inline under the
// toolbar, never over the TV screen, where the Engine window would hide it.
import { useEffect, useState } from "react";
import { Button } from "@/components/ui/button";
import * as commands from "@/lib/commands";

export function RunningSessions({
  onAdopt,
  onClose,
  onError,
}: {
  onAdopt: (id: string) => void;
  onClose: () => void;
  onError: (error: unknown) => void;
}) {
  const [sessions, setSessions] = useState<commands.RunningSession[] | null>(null);

  useEffect(() => {
    commands.runningSessions().then(setSessions, onError);
  }, [onError]);

  return (
    <div className="flex flex-col gap-2 border-b px-3 py-2 text-sm">
      <div className="flex items-center gap-2">
        <span className="font-medium">Running Sessions</span>
        <span className="text-muted-foreground">started with tizen-sim run</span>
        <Button variant="ghost" size="sm" className="ml-auto" onClick={onClose}>
          Close
        </Button>
      </div>
      {sessions === null ? (
        <p className="text-muted-foreground">Looking…</p>
      ) : sessions.length === 0 ? (
        <p className="text-muted-foreground">No Session is running outside the GUI.</p>
      ) : (
        <ul aria-label="Running Sessions" className="flex flex-col gap-1">
          {sessions.map((s) => (
            <li key={s.id} className="flex items-center gap-3">
              <span className="font-mono text-xs text-muted-foreground">{s.id.slice(0, 8)}</span>
              <span className="font-medium">{s.app_id}</span>
              <span>Tizen {s.profile}</span>
              <span className="truncate text-muted-foreground">{s.source}</span>
              <Button variant="outline" size="sm" className="ml-auto" onClick={() => onAdopt(s.id)}>
                Adopt
              </Button>
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}

// The Session's log, Network Log and Diagnostics.
import { useEffect, useRef } from "react";
import { Badge } from "@/components/ui/badge";
import type { LogLine, SessionView } from "@/lib/session";
import { cn } from "@/lib/utils";

const levelStyle: Record<LogLine["level"], string> = {
  debug: "text-muted-foreground",
  info: "",
  warning: "text-amber-600 dark:text-amber-400",
  error: "text-red-600 dark:text-red-400",
};

const sourceLabel: Record<LogLine["source"], string> = {
  console: "console",
  key: "key",
  session: "sim",
};

/** Keeps a scrolling list pinned to its end while the user is at the end. */
function useFollow<T extends HTMLElement>(length: number) {
  const ref = useRef<T>(null);
  const atEnd = useRef(true);
  useEffect(() => {
    const el = ref.current;
    if (el && atEnd.current) el.scrollTop = el.scrollHeight;
  }, [length]);
  const onScroll = () => {
    const el = ref.current;
    if (el) atEnd.current = el.scrollHeight - el.scrollTop - el.clientHeight < 24;
  };
  return { ref, onScroll };
}

export function LogPanel({ log }: { log: LogLine[] }) {
  const follow = useFollow<HTMLOListElement>(log.length);
  if (log.length === 0) return <Empty>Console output, Remote Keys and Session events appear here.</Empty>;
  return (
    <ol {...follow} aria-label="Log" className="h-full overflow-auto font-mono text-xs leading-5">
      {log.map((line) => (
        <li key={line.id} className={cn("flex gap-2 border-b px-3 py-0.5", levelStyle[line.level])}>
          <span className="w-14 shrink-0 text-muted-foreground">{sourceLabel[line.source]}</span>
          <span className="whitespace-pre-wrap break-all">{line.text}</span>
        </li>
      ))}
    </ol>
  );
}

export function NetworkPanel({ requests }: { requests: SessionView["network"] }) {
  const follow = useFollow<HTMLDivElement>(requests.length);
  if (requests.length === 0) return <Empty>Requests the App makes over the network appear here.</Empty>;
  return (
    <div {...follow} className="h-full overflow-auto">
      <table aria-label="Network Log" className="w-full font-mono text-xs">
        <thead className="sticky top-0 bg-background text-left text-muted-foreground">
          <tr>
            <th className="px-3 py-1 font-normal">Verdict</th>
            <th className="px-3 py-1 font-normal">Method</th>
            <th className="px-3 py-1 font-normal">URL</th>
            <th className="px-3 py-1 font-normal">Status</th>
            <th className="px-3 py-1 text-right font-normal">Time</th>
          </tr>
        </thead>
        <tbody>
          {requests.map((r) => {
            const missing = r.verdict === "Allowed" ? null : r.verdict.Blocked.missing_access;
            return (
              <tr key={r.id} className={cn("border-t", missing && "text-red-600 dark:text-red-400")}>
                <td className="px-3 py-1">
                  {missing ? (
                    <Badge variant="destructive">Blocked</Badge>
                  ) : (
                    <Badge variant="secondary">Allowed</Badge>
                  )}
                </td>
                <td className="px-3 py-1">{r.method}</td>
                <td className="px-3 py-1 break-all">
                  {r.url}
                  {missing && <div className="text-muted-foreground">needs {missing}</div>}
                </td>
                <td className="px-3 py-1">{r.status ?? "—"}</td>
                <td className="px-3 py-1 text-right">{Math.round(r.duration.secs * 1000 + r.duration.nanos / 1e6)} ms</td>
              </tr>
            );
          })}
        </tbody>
      </table>
    </div>
  );
}

export function DiagnosticsPanel({ diagnostics }: { diagnostics: SessionView["diagnostics"] }) {
  if (diagnostics.length === 0) {
    return <Empty>Nothing so far would behave differently on a TV.</Empty>;
  }
  return (
    <ul aria-label="Diagnostics" className="h-full overflow-auto">
      {diagnostics.map((d) => (
        <li key={d.id} className="border-b px-3 py-2 text-sm">
          <code className="text-xs text-amber-600 dark:text-amber-400">{d.code}</code>
          <p>{d.message}</p>
        </li>
      ))}
    </ul>
  );
}

function Empty({ children }: { children: string }) {
  return <p className="p-6 text-center text-sm text-muted-foreground">{children}</p>;
}

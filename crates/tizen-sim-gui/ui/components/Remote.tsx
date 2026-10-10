// The on-screen remote. Buttons carry Tizen Remote Key names
// (crates/tizen-sim-core/data/remote_keys.toml); the Session decides whether
// each press is delivered or dropped, as a TV would.
import {
  ArrowDown,
  ArrowLeft,
  ArrowRight,
  ArrowUp,
  FastForward,
  Pause,
  Play,
  Rewind,
  SkipBack,
  SkipForward,
  Square,
  Circle,
  Undo2,
  Volume1,
  Volume2,
  VolumeX,
  ChevronUp,
  ChevronDown,
  LogOut,
} from "lucide-react";
import type { ReactNode } from "react";
import { Button } from "@/components/ui/button";
import { cn } from "@/lib/utils";

type Key = { name: string; label: ReactNode; title?: string; className?: string };

const icon = (Icon: typeof ArrowUp) => <Icon className="size-4" />;

const colorKeys: Key[] = [
  { name: "ColorF0Red", label: "", title: "Red", className: "bg-red-600 hover:bg-red-500" },
  { name: "ColorF1Green", label: "", title: "Green", className: "bg-green-600 hover:bg-green-500" },
  { name: "ColorF2Yellow", label: "", title: "Yellow", className: "bg-yellow-400 hover:bg-yellow-300" },
  { name: "ColorF3Blue", label: "", title: "Blue", className: "bg-blue-600 hover:bg-blue-500" },
];

const digits: Key[] = [
  ..."123456789".split("").map((d) => ({ name: d, label: d })),
  { name: "Minus", label: "-" },
  { name: "0", label: "0" },
  { name: "PreviousChannel", label: "Pre-Ch", title: "PreviousChannel" },
];

const media: Key[] = [
  { name: "MediaRewind", label: icon(Rewind), title: "MediaRewind" },
  { name: "MediaPlay", label: icon(Play), title: "MediaPlay" },
  { name: "MediaPause", label: icon(Pause), title: "MediaPause" },
  { name: "MediaFastForward", label: icon(FastForward), title: "MediaFastForward" },
  { name: "MediaTrackPrevious", label: icon(SkipBack), title: "MediaTrackPrevious" },
  {
    name: "MediaPlayPause",
    label: (
      <>
        <Play className="size-3" />
        <Pause className="-ml-1.5 size-3" />
      </>
    ),
    title: "MediaPlayPause",
  },
  { name: "MediaStop", label: icon(Square), title: "MediaStop" },
  { name: "MediaTrackNext", label: icon(SkipForward), title: "MediaTrackNext" },
  { name: "MediaRecord", label: icon(Circle), title: "MediaRecord" },
];

const volumeAndChannel: Key[] = [
  { name: "VolumeUp", label: icon(Volume2), title: "VolumeUp" },
  { name: "VolumeMute", label: icon(VolumeX), title: "VolumeMute" },
  { name: "ChannelUp", label: icon(ChevronUp), title: "ChannelUp" },
  { name: "VolumeDown", label: icon(Volume1), title: "VolumeDown" },
  { name: "ChannelList", label: "CH List", title: "ChannelList" },
  { name: "ChannelDown", label: icon(ChevronDown), title: "ChannelDown" },
];

/** Keys without a place of their own on the remote above. */
const more = [
  "Menu",
  "Tools",
  "Info",
  "Guide",
  "Source",
  "Search",
  "Caption",
  "E-Manual",
  "3D",
  "Extra",
  "PictureSize",
  "Soccer",
  "Teletext",
  "MTS",
].map((name) => ({ name, label: name }));

export function Remote({ onPress, disabled }: { onPress: (key: string) => void; disabled: boolean }) {
  const key = (k: Key, size: "icon" | "sm" = "icon") => (
    <Button
      key={k.name}
      variant="secondary"
      size={size}
      title={k.title ?? k.name}
      aria-label={k.title ?? k.name}
      disabled={disabled}
      onClick={() => onPress(k.name)}
      className={cn("w-full", k.className)}
    >
      {k.label}
    </Button>
  );

  return (
    <div className="flex flex-col gap-4" aria-label="Remote">
      <div className="grid grid-cols-2 gap-2">
        {key({ name: "Back", label: <>{icon(Undo2)} Back</> }, "sm")}
        {key({ name: "Exit", label: <>{icon(LogOut)} Exit</> }, "sm")}
      </div>

      <div className="grid grid-cols-3 gap-2">
        <span />
        {key({ name: "ArrowUp", label: icon(ArrowUp), title: "ArrowUp" })}
        <span />
        {key({ name: "ArrowLeft", label: icon(ArrowLeft), title: "ArrowLeft" })}
        {key({ name: "Enter", label: "OK", title: "Enter" })}
        {key({ name: "ArrowRight", label: icon(ArrowRight), title: "ArrowRight" })}
        <span />
        {key({ name: "ArrowDown", label: icon(ArrowDown), title: "ArrowDown" })}
        <span />
      </div>

      <div className="grid grid-cols-4 gap-2">{colorKeys.map((k) => key(k, "sm"))}</div>
      <div className="grid grid-cols-3 gap-2">{volumeAndChannel.map((k) => key(k))}</div>
      <div className="grid grid-cols-3 gap-2">{digits.map((k) => key(k, "sm"))}</div>
      <div className="grid grid-cols-4 gap-2">{media.map((k) => key(k))}</div>
      <div className="grid grid-cols-2 gap-2">{more.map((k) => key(k, "sm"))}</div>
    </div>
  );
}

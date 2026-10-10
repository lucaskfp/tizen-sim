// The GUI's Rust commands and the Session events it emits (src/lib.rs).
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type { GuiEvent, KeyOutcome } from "./session";

export type SourceChoice =
  | { kind: "path"; path: string }
  | { kind: "devServer"; url: string; config: string };

export type ProfileInfo = { version: string; milestone: number; latest: boolean };

export const openApp = (source: SourceChoice) => invoke<void>("open_app", { source });
export const switchProfile = (profile: string) => invoke<void>("switch_profile", { profile });
export const pressKey = (key: string) => invoke<KeyOutcome>("press_key", { key });
export const reload = () => invoke<void>("reload");
export const screenshot = (path: string) => invoke<void>("screenshot", { path });
export const profiles = () => invoke<ProfileInfo[]>("profiles");

export const onSessionEvent = (handle: (event: GuiEvent) => void): Promise<UnlistenFn> =>
  // SESSION_EVENT in src/lib.rs.
  listen<GuiEvent>("session-event", (e) => handle(e.payload));

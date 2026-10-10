// The GUI's Rust commands and the Session events it emits (src/lib.rs).
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type { GuiEvent, KeyOutcome } from "./session";

export type SourceChoice =
  | { kind: "path"; path: string }
  | { kind: "devServer"; url: string; config: string };

export type ProfileInfo = { version: string; milestone: number; latest: boolean };

/** A Session another process (the CLI) runs, which the GUI can adopt. */
export type RunningSession = { id: string; app_id: string; profile: string; source: string };

/** The screen area's box in CSS px of the GUI's webview. */
export type ScreenArea = { x: number; y: number; width: number; height: number };

export const openApp = (source: SourceChoice) => invoke<void>("open_app", { source });
export const switchProfile = (profile: string) => invoke<void>("switch_profile", { profile });
export const pressKey = (key: string) => invoke<KeyOutcome>("press_key", { key });
export const reload = () => invoke<void>("reload");
export const screenshot = (path: string) => invoke<void>("screenshot", { path });
export const profiles = () => invoke<ProfileInfo[]>("profiles");
export const restart = () => invoke<void>("restart");
export const runningSessions = () => invoke<RunningSession[]>("running_sessions");
export const adopt = (id: string) => invoke<void>("adopt", { id });
/** Whether the TV screen is shown inside the GUI (on Windows). */
export const embedsScreen = () => invoke<boolean>("embeds_screen");
export const setScreenArea = (area: ScreenArea | null) => invoke<void>("set_screen_area", { area });

export const onSessionEvent = (handle: (event: GuiEvent) => void): Promise<UnlistenFn> =>
  // SESSION_EVENT in src/lib.rs.
  listen<GuiEvent>("session-event", (e) => handle(e.payload));

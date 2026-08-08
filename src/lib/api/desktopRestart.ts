import { invoke } from "@tauri-apps/api/core";

/** Desktop apps that can be restarted from CC Switch */
export type DesktopAppTarget = "claude" | "codex";

export interface RestartDesktopAppResult {
  /** Whether the app was running before the restart */
  wasRunning: boolean;
  /** Whether a new instance was launched */
  launched: boolean;
  /** Executable / bundle path actually used */
  path: string | null;
}

export const desktopRestartApi = {
  /**
   * Kill and relaunch a desktop app so it picks up the current config.
   * Desktop clients read their config at startup only.
   */
  async restart(target: DesktopAppTarget): Promise<RestartDesktopAppResult> {
    return await invoke("restart_desktop_app", { target });
  },
};

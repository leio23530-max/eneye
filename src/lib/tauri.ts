import { getCurrentWindow } from "@tauri-apps/api/window";

export function isTauriRuntime(): boolean {
  return typeof window !== "undefined" && Boolean(window.__TAURI_INTERNALS__);
}

export function getWindowKind(): "overlay" | "settings" {
  if (
    typeof window !== "undefined" &&
    new URLSearchParams(window.location.search).get("window") === "overlay"
  ) {
    return "overlay";
  }
  if (!isTauriRuntime()) return "settings";

  return getCurrentWindow().label === "overlay" ? "overlay" : "settings";
}

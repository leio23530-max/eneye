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

  const internals = window.__TAURI_INTERNALS__ as {
    metadata?: { currentWindow?: { label?: string } };
  };
  return internals.metadata?.currentWindow?.label === "overlay"
    ? "overlay"
    : "settings";
}

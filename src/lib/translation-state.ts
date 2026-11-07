import type { TranslationEvent, TranslationState } from "../types";

export function applyTranslationEvent(
  current: TranslationState | null,
  event: TranslationEvent,
): TranslationState | null {
  if (!current || event.requestId !== current.requestId) return current;
  switch (event.type) {
    case "started":
      return current.status === "streaming"
        ? current
        : { ...current, status: "streaming", error: undefined };
    case "delta":
      return {
        ...current,
        status: "streaming",
        output: current.output + event.text,
      };
    case "completed":
      return { ...current, status: "complete", output: event.text };
    case "failed":
      return { ...current, status: "error", error: event.error };
    case "cancelled":
      return { ...current, status: "cancelled", error: undefined };
    default:
      return current;
  }
}

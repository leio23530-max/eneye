import type { TranslationEvent, TranslationState } from "../types";

export function applyTranslationFailure(
  current: TranslationState | null,
  requestId: string,
  error: unknown,
): TranslationState | null {
  if (!current || current.requestId !== requestId) return current;
  if (current.status === "complete") return current;
  const message = String(error);
  if (message === "translation cancelled") {
    return { ...current, status: "cancelled", error: undefined };
  }
  return { ...current, status: "error", error: message };
}

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

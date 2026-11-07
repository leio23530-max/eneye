import { describe, expect, it } from "vitest";
import type { TranslationState } from "../types";
import {
  applyTranslationEvent,
  applyTranslationFailure,
} from "./translation-state";

const current: TranslationState = {
  requestId: "request-1",
  text: "pollution",
  kind: "translate",
  status: "streaming",
  output: "污",
};

describe("applyTranslationEvent", () => {
  it("assembles streamed deltas and completes with the final text", () => {
    const first = applyTranslationEvent(current, {
      type: "delta",
      requestId: "request-1",
      text: "染",
    });
    expect(first?.output).toBe("污染");
    const done = applyTranslationEvent(first, {
      type: "completed",
      requestId: "request-1",
      text: "污染",
    });
    expect(done).toMatchObject({ status: "complete", output: "污染" });
  });

  it("treats cancellation as a neutral terminal state with partial text", () => {
    const cancelled = applyTranslationEvent(current, {
      type: "cancelled",
      requestId: "request-1",
    });
    expect(cancelled).toMatchObject({
      status: "cancelled",
      output: "污",
      error: undefined,
    });
  });

  it("ignores events from a stale request", () => {
    const stale = applyTranslationEvent(current, {
      type: "completed",
      requestId: "request-0",
      text: "旧译文",
    });
    expect(stale).toBe(current);
  });

  it("keeps provider errors visible", () => {
    const failed = applyTranslationEvent(current, {
      type: "failed",
      requestId: "request-1",
      error: "API Key 无效",
    });
    expect(failed).toMatchObject({ status: "error", error: "API Key 无效" });
  });
});

describe("applyTranslationFailure", () => {
  it("turns a cancelled command rejection into the neutral stopped state", () => {
    const failed = applyTranslationFailure(
      current,
      "request-1",
      "translation cancelled",
    );
    expect(failed).toMatchObject({
      status: "cancelled",
      output: "污",
      error: undefined,
    });
  });

  it("keeps a provider error visible", () => {
    const failed = applyTranslationFailure(
      current,
      "request-1",
      "API 服务返回 401",
    );
    expect(failed).toMatchObject({
      status: "error",
      error: "API 服务返回 401",
    });
  });

  it("ignores a failure from a stale request", () => {
    expect(applyTranslationFailure(current, "request-0", "旧错误")).toBe(
      current,
    );
  });

  it("does not downgrade a completed translation", () => {
    const complete: TranslationState = {
      ...current,
      status: "complete",
      output: "污染",
    };
    expect(
      applyTranslationFailure(complete, "request-1", "translation cancelled"),
    ).toBe(complete);
  });
});

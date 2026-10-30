import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { DEFAULT_SETTINGS } from "../data/sample";
import type { TranslationState } from "../types";
import { TranslationOverlayCard } from "./OverlayWindow";

const translation: TranslationState = {
  requestId: "request-1",
  text: "A quiet interface should disappear into reading.",
  status: "complete",
  output: "安静的界面应当消融于阅读之中。",
};

describe("TranslationOverlayCard", () => {
  it("uses explicit close instead of an ambiguous pin action", async () => {
    const user = userEvent.setup();
    const onClose = vi.fn();
    render(
      <TranslationOverlayCard
        onCancel={() => undefined}
        onClose={onClose}
        onRetry={() => undefined}
        settings={DEFAULT_SETTINGS}
        translation={translation}
      />,
    );
    expect(screen.queryByRole("button", { name: "固定卡片" })).toBeNull();
    await user.click(screen.getByRole("button", { name: "关闭卡片" }));
    expect(onClose).toHaveBeenCalledOnce();
  });

  it("keeps retry and stop actions available in their states", () => {
    const { rerender } = render(
      <TranslationOverlayCard
        onCancel={() => undefined}
        onClose={() => undefined}
        onRetry={() => undefined}
        settings={DEFAULT_SETTINGS}
        translation={{ ...translation, status: "streaming", output: "" }}
      />,
    );
    expect(screen.getByRole("button", { name: "停止翻译" })).toBeVisible();
    rerender(
      <TranslationOverlayCard
        onCancel={() => undefined}
        onClose={() => undefined}
        onRetry={() => undefined}
        settings={DEFAULT_SETTINGS}
        translation={{
          ...translation,
          status: "error",
          output: "",
          error: "网络失败",
        }}
      />,
    );
    expect(screen.getByRole("button", { name: "重试" })).toBeVisible();
  });
});

import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { DEFAULT_SETTINGS } from "../data/sample";
import type { TranslationState } from "../types";
import { TranslationOverlayCard } from "./OverlayWindow";

const translation: TranslationState = {
  requestId: "request-1",
  text: "A quiet interface should disappear into reading.",
  kind: "translate",
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

  it("labels an explanation as explanation rather than translation", () => {
    render(
      <TranslationOverlayCard
        onCancel={() => undefined}
        onClose={() => undefined}
        onRetry={() => undefined}
        settings={DEFAULT_SETTINGS}
        translation={{
          ...translation,
          kind: "explain",
          status: "streaming",
          output: "",
        }}
      />,
    );
    expect(screen.getByText("AI EXPLANATION")).toBeVisible();
    expect(screen.getByRole("heading", { name: "正在解释" })).toBeVisible();
    expect(screen.getByRole("button", { name: "停止解释" })).toBeVisible();
  });

  it("shows cancellation as a neutral stopped state, not an error", () => {
    render(
      <TranslationOverlayCard
        onCancel={() => undefined}
        onClose={() => undefined}
        onRetry={() => undefined}
        settings={DEFAULT_SETTINGS}
        translation={{
          ...translation,
          status: "cancelled",
          output: "安静的界面",
        }}
      />,
    );
    expect(screen.getByRole("heading", { name: "已停止翻译" })).toBeVisible();
    expect(screen.getByText("安静的界面")).toBeVisible();
    expect(screen.getByText("已停止，保留已经生成的内容。")).toBeVisible();
    expect(screen.queryByRole("alert")).toBeNull();
    expect(screen.getByRole("button", { name: "重新翻译" })).toBeVisible();
  });
});

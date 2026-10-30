import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { OverlayCard } from "./OverlayCard";
import {
  DEFAULT_SETTINGS,
  SAMPLE_LOOKUP,
  SAMPLE_TRANSLATION,
} from "../data/sample";

describe("OverlayCard", () => {
  it("shows the primary word and expands secondary dictionaries", async () => {
    const user = userEvent.setup();
    render(
      <OverlayCard
        lookup={SAMPLE_LOOKUP}
        settings={DEFAULT_SETTINGS}
        translation={SAMPLE_TRANSLATION}
      />,
    );

    expect(screen.getByRole("heading", { name: "serendipity" })).toBeVisible();
    expect(screen.getByText("意外发现美好事物的运气；机缘巧合")).toBeVisible();
    expect(
      screen.getByText("安静的界面应当消融于阅读行为本身。"),
    ).toBeVisible();

    const accordion = screen.getByRole("button", {
      name: "English WordNet",
    });
    expect(accordion).toHaveAttribute("aria-expanded", "false");
    await user.click(accordion);
    expect(accordion).toHaveAttribute("aria-expanded", "true");
  });

  it("pins the card and reports the pinned state", async () => {
    const user = userEvent.setup();
    const onPinChange = vi.fn();
    render(
      <OverlayCard
        lookup={SAMPLE_LOOKUP}
        onPinChange={onPinChange}
        settings={DEFAULT_SETTINGS}
      />,
    );

    await user.click(screen.getByRole("button", { name: "固定卡片" }));
    expect(onPinChange).toHaveBeenCalledWith(true);
    expect(screen.getByRole("button", { name: "取消固定" })).toHaveClass(
      "is-active",
    );
  });

  it("exposes retry when translation fails", () => {
    render(
      <OverlayCard
        lookup={SAMPLE_LOOKUP}
        settings={DEFAULT_SETTINGS}
        translation={{
          ...SAMPLE_TRANSLATION,
          status: "error",
          output: "",
          error: "网络连接失败",
        }}
      />,
    );
    expect(screen.getByRole("alert")).toHaveTextContent("网络连接失败");
    expect(screen.getByRole("button", { name: "重试" })).toBeVisible();
  });

  it("does not jump away from the dictionary on initial render", () => {
    const scrollTo = vi
      .spyOn(HTMLElement.prototype, "scrollTo")
      .mockImplementation(() => undefined);
    render(
      <OverlayCard
        lookup={SAMPLE_LOOKUP}
        settings={DEFAULT_SETTINGS}
        translation={SAMPLE_TRANSLATION}
      />,
    );
    expect(scrollTo).not.toHaveBeenCalled();
    scrollTo.mockRestore();
  });
});

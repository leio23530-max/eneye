import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it } from "vitest";
import { DictionarySearchPanel } from "./DictionarySearchPanel";

describe("DictionarySearchPanel", () => {
  it("searches with a Chinese query", async () => {
    const user = userEvent.setup();
    render(<DictionarySearchPanel />);

    await user.type(screen.getByLabelText("词典搜索"), "美丽");
    await user.click(screen.getByRole("button", { name: "查询" }));

    expect(await screen.findByText("“美丽” · 2 条结果")).toBeVisible();
    expect(screen.getByText("意外发现美好事物的运气；机缘巧合")).toBeVisible();
    expect(screen.getByText("ECDICT")).toBeVisible();
  });
});

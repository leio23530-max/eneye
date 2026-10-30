import { fireEvent, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { useState } from "react";
import { describe, expect, it } from "vitest";
import { DEFAULT_SETTINGS } from "../data/sample";
import type { AppSettings } from "../types";
import { SettingsWindow } from "./SettingsWindow";

function Harness() {
  const [settings, setSettings] = useState<AppSettings>(DEFAULT_SETTINGS);
  return (
    <SettingsWindow
      installedFonts={["Arial"]}
      onSettingsChange={setSettings}
      settings={settings}
    />
  );
}

describe("SettingsWindow", () => {
  it("adjusts hover delay and persists the selected value", async () => {
    const user = userEvent.setup();
    render(<Harness />);

    const slider = screen.getByRole("slider", { name: "悬浮停留时间" });
    fireEvent.change(slider, { target: { value: "200" } });
    expect(slider).toHaveValue("200");
    expect(screen.getByText("200ms")).toBeVisible();
  });

  it("lets the user choose an installed font", async () => {
    const user = userEvent.setup();
    render(<Harness />);

    const select = screen.getByRole("combobox", {
      name: "英文词头字体",
    });
    await user.selectOptions(select, "Arial");
    expect(select).toHaveValue("Arial");
  });

  it("switches between settings sections", async () => {
    const user = userEvent.setup();
    render(<Harness />);

    await user.click(screen.getByRole("button", { name: "AI 翻译" }));
    expect(screen.getByRole("heading", { name: "AI 翻译设置" })).toBeVisible();
  });
});

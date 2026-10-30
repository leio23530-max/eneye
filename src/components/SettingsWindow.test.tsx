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
      agentConnections={[
        {
          id: "codex",
          name: "Codex",
          available: true,
          detail: "已检测到 codex 本地命令",
        },
        {
          id: "claude-code",
          name: "Claude Code",
          available: false,
          detail: "未在 PATH 中检测到 claude",
        },
        {
          id: "kimi-code",
          name: "Kimi Code",
          available: false,
          detail: "未在 PATH 中检测到 kimi",
        },
      ]}
      aiKeyConfigured={false}
      dictionaryStatus={{
        available: true,
        name: "ECDICT",
        entryCount: 770_612,
        path: "fixture",
      }}
      externalDictionaries={[]}
      installedFonts={["Arial"]}
      ocrAvailable
      onAiKeyConfiguredChange={() => undefined}
      onExternalDictionariesChange={() => undefined}
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

  it("keeps the word card linger time configurable", async () => {
    render(<Harness />);
    const slider = screen.getByRole("slider", { name: "移开后保留时间" });
    fireEvent.change(slider, { target: { value: "2200" } });
    expect(slider).toHaveValue("2200");
    expect(screen.getByText("当前 2200ms")).toBeVisible();
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
    expect(screen.getByRole("heading", { name: "Agent 连接" })).toBeVisible();
  });

  it("saves a new AI key through the credential callback", async () => {
    const user = userEvent.setup();
    render(<Harness />);
    await user.click(screen.getByRole("button", { name: "AI 翻译" }));
    await user.click(screen.getByRole("button", { name: /API/ }));
    await user.type(screen.getByLabelText("API Key"), "test-key");
    await user.click(screen.getByRole("button", { name: "保存密钥" }));
    expect(screen.getByRole("button", { name: "已保存" })).toBeVisible();
  });

  it("applies NeoRead-style provider protocols", async () => {
    const user = userEvent.setup();
    render(<Harness />);
    await user.click(screen.getByRole("button", { name: "AI 翻译" }));
    await user.click(screen.getByRole("button", { name: /API/ }));
    await user.click(screen.getByRole("button", { name: "Anthropic" }));
    expect(screen.getByLabelText("AI 接口协议")).toHaveValue("anthropic");
    expect(screen.getByLabelText("AI Base URL")).toHaveValue(
      "https://api.anthropic.com",
    );
  });

  it("shows the NeoRead connection cards and role rows", async () => {
    const user = userEvent.setup();
    render(<Harness />);
    await user.click(screen.getByRole("button", { name: "AI 翻译" }));
    expect(screen.getByText("Agent 连接")).toBeVisible();
    expect(screen.getByText("Claude Code")).toBeVisible();
    expect(screen.getByText("Kimi Code")).toBeVisible();
    expect(screen.getByText("已检测到 codex 本地命令")).toBeVisible();
    expect(screen.getAllByText("未检测到")).toHaveLength(2);
    expect(screen.getByLabelText("问答模型")).toBeVisible();
    expect(screen.getByLabelText("翻译模型")).toBeVisible();
    expect(screen.getByLabelText("图片模型")).toBeVisible();
  });
});

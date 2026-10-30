import { useMemo, useState } from "react";
import { FONT_OPTIONS } from "../data/sample";
import type { AppSettings } from "../types";
import { Icon } from "./Icon";

type Section = "general" | "dictionary" | "ai" | "history" | "diagnostics";

const sections: Array<{
  id: Section;
  label: string;
  icon: Parameters<typeof Icon>[0]["name"];
}> = [
  { id: "general", label: "通用", icon: "book" },
  { id: "dictionary", label: "词典", icon: "folder" },
  { id: "ai", label: "AI 翻译", icon: "sparkles" },
  { id: "history", label: "历史", icon: "history" },
  { id: "diagnostics", label: "诊断", icon: "pulse" },
];

interface SettingsWindowProps {
  settings: AppSettings;
  onSettingsChange: (settings: AppSettings) => void;
  installedFonts?: string[];
}

function Toggle({
  checked,
  label,
  onChange,
}: {
  checked: boolean;
  label: string;
  onChange: (checked: boolean) => void;
}) {
  return (
    <button
      aria-checked={checked}
      aria-label={label}
      className="toggle"
      onClick={() => onChange(!checked)}
      role="switch"
      type="button"
    >
      <span />
    </button>
  );
}

function FontSelect({
  label,
  value,
  options,
  onChange,
}: {
  label: string;
  value: string;
  options: string[];
  onChange: (value: string) => void;
}) {
  return (
    <label className="field-row">
      <span>
        <span className="field-label">{label}</span>
        <span className="field-help">使用系统已安装字体，无需联网下载</span>
      </span>
      <select
        aria-label={label}
        onChange={(event) => onChange(event.target.value)}
        value={value}
      >
        {options.map((font) => (
          <option key={font} value={font} style={{ fontFamily: font }}>
            {font}
          </option>
        ))}
      </select>
    </label>
  );
}

export function SettingsWindow({
  settings,
  onSettingsChange,
  installedFonts = [],
}: SettingsWindowProps) {
  const [section, setSection] = useState<Section>("general");
  const [sampleActive, setSampleActive] = useState(false);
  const fontOptions = useMemo(
    () =>
      Array.from(
        new Set([
          ...FONT_OPTIONS.map((font) => font.family),
          ...installedFonts,
        ]),
      ).sort((a, b) => a.localeCompare(b, "zh-CN")),
    [installedFonts],
  );

  const update = <K extends keyof AppSettings>(key: K, value: AppSettings[K]) =>
    onSettingsChange({ ...settings, [key]: value });

  return (
    <main
      className="settings-shell"
      style={
        {
          "--body-font": `"${settings.bodyFont}", "Segoe UI", "Microsoft YaHei UI", sans-serif`,
          "--headword-font": `"${settings.headwordFont}", Georgia, serif`,
        } as React.CSSProperties
      }
    >
      <aside className="sidebar">
        <div className="brand">
          <span className="brand-mark" aria-hidden="true">
            e
          </span>
          <div>
            <strong>Eneye</strong>
            <span>轻读，随手可译</span>
          </div>
        </div>

        <nav aria-label="设置导航">
          {sections.map((item) => (
            <button
              aria-current={section === item.id ? "page" : undefined}
              className={section === item.id ? "is-active" : ""}
              key={item.id}
              onClick={() => setSection(item.id)}
              type="button"
            >
              <Icon name={item.icon} />
              {item.label}
            </button>
          ))}
        </nav>

        <div className="sidebar-footer">
          <span className="status-dot" />
          划词已开启
          <kbd>Alt Shift E</kbd>
        </div>
      </aside>

      <section className="settings-content">
        <header className="page-header">
          <div>
            <p className="eyebrow">ENEYE PREFERENCES</p>
            <h1>{sections.find((item) => item.id === section)?.label}</h1>
          </div>
          <div className="quiet-note">
            <Icon name="check" size={16} />
            设置自动保存
          </div>
        </header>

        {section === "general" && (
          <div className="settings-stack">
            <section className="settings-section">
              <div className="section-title">
                <div>
                  <h2>取词手感</h2>
                  <p>提前解析文字，只在鼠标稳定后展示结果。</p>
                </div>
                <span className="value-pill">{settings.lookupDelayMs}ms</span>
              </div>
              <label className="range-field">
                <span>悬浮停留时间</span>
                <input
                  aria-label="悬浮停留时间"
                  max="500"
                  min="150"
                  onChange={(event) =>
                    update("lookupDelayMs", Number(event.target.value))
                  }
                  step="10"
                  type="range"
                  value={settings.lookupDelayMs}
                />
                <span className="range-labels">
                  <span>灵敏 150ms</span>
                  <span>稳定 500ms</span>
                </span>
              </label>
              <button
                className={`hover-preview ${sampleActive ? "is-active" : ""}`}
                onClick={() => setSampleActive(true)}
                onMouseEnter={() => setSampleActive(true)}
                type="button"
              >
                <span className="preview-line">
                  A quiet place makes room for <mark>serendipity</mark>.
                </span>
                <span className="preview-caption">
                  把鼠标停留在高亮单词上预览
                </span>
                {sampleActive && (
                  <span className="preview-card" aria-live="polite">
                    <strong>serendipity</strong>
                    <small>/ˌserənˈdɪpəti/</small>
                    <span>意外发现美好事物的运气</span>
                  </span>
                )}
              </button>
            </section>

            <section className="settings-section">
              <div className="section-title">
                <div>
                  <h2>行为</h2>
                  <p>Eneye 保持安静，只在需要时出现。</p>
                </div>
              </div>
              <div className="setting-row">
                <span>
                  <span className="field-label">自动翻译选区</span>
                  <span className="field-help">
                    至少两个拉丁词或四个 CJK 字符
                  </span>
                </span>
                <Toggle
                  checked={settings.autoTranslate}
                  label="自动翻译选区"
                  onChange={(value) => update("autoTranslate", value)}
                />
              </div>
              <div className="setting-row">
                <span>
                  <span className="field-label">开机自动启动</span>
                  <span className="field-help">默认关闭，不后台扫描文件</span>
                </span>
                <Toggle
                  checked={settings.launchAtStartup}
                  label="开机自动启动"
                  onChange={(value) => update("launchAtStartup", value)}
                />
              </div>
            </section>

            <section className="settings-section">
              <div className="section-title">
                <div>
                  <h2>字体</h2>
                  <p>来自本机字体库，选择后立即应用。</p>
                </div>
                <Icon name="font" />
              </div>
              <FontSelect
                label="英文词头字体"
                onChange={(value) => update("headwordFont", value)}
                options={fontOptions}
                value={settings.headwordFont}
              />
              <FontSelect
                label="正文与界面字体"
                onChange={(value) => update("bodyFont", value)}
                options={fontOptions}
                value={settings.bodyFont}
              />
              <div className="font-preview">
                <strong>serendipity</strong>
                <span>安静、清晰、恰到好处的阅读体验。</span>
              </div>
            </section>
          </div>
        )}

        {section === "dictionary" && (
          <PlaceholderPanel
            description="首版会内置完整 ECDICT，并支持 CSV、StarDict 与 MDX/MDD。"
            icon="folder"
            title="词典管理将在下一提交接入"
          />
        )}
        {section === "ai" && (
          <PlaceholderPanel
            description="OpenAI 兼容接口将在下一提交接入，API Key 只保存到 Windows 凭据管理器。"
            icon="sparkles"
            title="AI 翻译设置"
          />
        )}
        {section === "history" && (
          <PlaceholderPanel
            description="查询历史保存在本机，最多保留 10,000 条，不保存截图或窗口标题。"
            icon="history"
            title="本地查询历史"
          />
        )}
        {section === "diagnostics" && (
          <PlaceholderPanel
            description="这里将显示 OCR、鼠标取词、WebView2 和词典状态。"
            icon="pulse"
            title="运行诊断"
          />
        )}
      </section>
    </main>
  );
}

function PlaceholderPanel({
  icon,
  title,
  description,
}: {
  icon: Parameters<typeof Icon>[0]["name"];
  title: string;
  description: string;
}) {
  return (
    <div className="empty-panel">
      <span className="empty-icon">
        <Icon name={icon} size={24} />
      </span>
      <h2>{title}</h2>
      <p>{description}</p>
    </div>
  );
}

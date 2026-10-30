import { useEffect, useMemo, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { openUrl } from "@tauri-apps/plugin-opener";
import { FONT_OPTIONS } from "../data/sample";
import {
  addExternalDictionary,
  clearHistory,
  deleteAiKey,
  deleteHistory,
  getHistory,
  listAiModels,
  removeExternalDictionary,
  reorderExternalDictionaries,
  setAiKey,
} from "../lib/api";
import { isTauriRuntime } from "../lib/tauri";
import type {
  AppSettings,
  DictionaryStatus,
  ExternalDictionary,
  HistoryRecord,
} from "../types";
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

const SOURCE_LANGUAGES = [
  { label: "自动识别", value: "auto" },
  { label: "英语", value: "English" },
  { label: "简体中文", value: "Simplified Chinese" },
  { label: "繁体中文", value: "Traditional Chinese" },
  { label: "日语", value: "Japanese" },
  { label: "韩语", value: "Korean" },
  { label: "法语", value: "French" },
  { label: "德语", value: "German" },
  { label: "西班牙语", value: "Spanish" },
  { label: "俄语", value: "Russian" },
];

const TARGET_LANGUAGES = [
  "简体中文",
  "繁体中文",
  "英语",
  "日语",
  "韩语",
  "法语",
  "德语",
  "西班牙语",
  "俄语",
  "阿拉伯语",
];

interface SettingsWindowProps {
  settings: AppSettings;
  onSettingsChange: (settings: AppSettings) => void;
  installedFonts?: string[];
  dictionaryStatus: DictionaryStatus;
  externalDictionaries: ExternalDictionary[];
  aiKeyConfigured: boolean;
  ocrAvailable: boolean;
  onAiKeyConfiguredChange: (configured: boolean) => void;
  onExternalDictionariesChange: (dictionaries: ExternalDictionary[]) => void;
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
  dictionaryStatus,
  externalDictionaries,
  aiKeyConfigured,
  ocrAvailable,
  onAiKeyConfiguredChange,
  onExternalDictionariesChange,
}: SettingsWindowProps) {
  const [section, setSection] = useState<Section>("general");
  const [sampleActive, setSampleActive] = useState(false);
  const [aiKey, setAiKeyValue] = useState("");
  const [keyStatus, setKeyStatus] = useState<"idle" | "saving" | "saved">(
    "idle",
  );
  const [keyError, setKeyError] = useState("");
  const [modelOptions, setModelOptions] = useState<string[]>([]);
  const [modelLoading, setModelLoading] = useState(false);
  const [modelError, setModelError] = useState("");
  const [history, setHistory] = useState<HistoryRecord[]>([]);
  const [historyQuery, setHistoryQuery] = useState("");
  const [historyLoading, setHistoryLoading] = useState(false);
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

  useEffect(() => {
    if (section !== "history") return;
    let active = true;
    setHistoryLoading(true);
    getHistory(historyQuery, 200)
      .then((records) => {
        if (active) setHistory(records);
      })
      .finally(() => {
        if (active) setHistoryLoading(false);
      });
    return () => {
      active = false;
    };
  }, [historyQuery, section]);

  async function saveAiCredential() {
    setKeyError("");
    setKeyStatus("saving");
    try {
      const configured = await setAiKey(aiKey);
      onAiKeyConfiguredChange(configured);
      setAiKeyValue("");
      setKeyStatus("saved");
      window.setTimeout(() => setKeyStatus("idle"), 1500);
    } catch (error) {
      setKeyStatus("idle");
      setKeyError(String(error));
    }
  }

  async function removeAiCredential() {
    setKeyError("");
    try {
      await deleteAiKey();
      onAiKeyConfiguredChange(false);
      setKeyStatus("idle");
    } catch (error) {
      setKeyError(String(error));
    }
  }

  function applyProviderPreset(provider: "deepseek" | "openai" | "anthropic") {
    const preset =
      provider === "deepseek"
        ? { baseUrl: "https://api.deepseek.com", protocol: "openai" as const }
        : provider === "openai"
          ? {
              baseUrl: "https://api.openai.com/v1",
              protocol: "openai" as const,
            }
          : {
              baseUrl: "https://api.anthropic.com",
              protocol: "anthropic" as const,
            };
    setModelOptions([]);
    onSettingsChange({
      ...settings,
      aiBaseUrl: preset.baseUrl,
      aiProtocol: preset.protocol,
    });
  }

  async function refreshModelOptions() {
    setModelError("");
    setModelLoading(true);
    try {
      const models = await listAiModels(settings, aiKey);
      setModelOptions(models);
      if (!settings.aiModel && models.length > 0) {
        update("aiModel", models[0]);
      }
    } catch (error) {
      setModelError(String(error));
    } finally {
      setModelLoading(false);
    }
  }

  async function removeHistoryRecord(id: number) {
    await deleteHistory(id);
    setHistory((records) => records.filter((record) => record.id !== id));
  }

  async function removeAllHistory() {
    await clearHistory();
    setHistory([]);
  }

  async function importDictionary() {
    if (!isTauriRuntime()) return;
    const selected = await open({
      multiple: false,
      directory: false,
      filters: [
        {
          name: "词典文件",
          extensions: ["csv", "mdx", "ifo", "idx", "dict", "dz", "syn"],
        },
      ],
    });
    if (typeof selected !== "string") return;
    const dictionary = await addExternalDictionary(selected);
    onExternalDictionariesChange([...externalDictionaries, dictionary]);
  }

  async function removeDictionary(id: string) {
    await removeExternalDictionary(id);
    onExternalDictionariesChange(
      externalDictionaries
        .filter((dictionary) => dictionary.id !== id)
        .map((dictionary, index) => ({ ...dictionary, priority: index })),
    );
  }

  async function moveDictionary(index: number, direction: -1 | 1) {
    const nextIndex = index + direction;
    if (nextIndex < 0 || nextIndex >= externalDictionaries.length) return;
    const reordered = [...externalDictionaries];
    [reordered[index], reordered[nextIndex]] = [
      reordered[nextIndex],
      reordered[index],
    ];
    const withPriorities = reordered.map((dictionary, priority) => ({
      ...dictionary,
      priority,
    }));
    onExternalDictionariesChange(withPriorities);
    await reorderExternalDictionaries(
      withPriorities.map((dictionary) => dictionary.id),
    );
  }

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
          <div className="settings-stack">
            <section className="settings-section">
              <div className="section-title">
                <div>
                  <h2>内置词典</h2>
                  <p>完整 ECDICT 随应用安装，查询不需要网络。</p>
                </div>
                <span
                  className={`status-pill ${
                    dictionaryStatus.available ? "is-ok" : "is-error"
                  }`}
                >
                  {dictionaryStatus.available ? "可用" : "不可用"}
                </span>
              </div>
              <div className="dictionary-source-card">
                <span className="dictionary-monogram">EC</span>
                <div>
                  <strong>{dictionaryStatus.name}</strong>
                  <span>
                    {dictionaryStatus.available
                      ? `${dictionaryStatus.entryCount.toLocaleString()} 个词条`
                      : dictionaryStatus.error}
                  </span>
                </div>
                <Icon name="check" />
              </div>
            </section>
            <section className="settings-section">
              <div className="section-title">
                <div>
                  <h2>添加词典</h2>
                  <p>支持 CSV、StarDict 和 MDX/MDD，文件保留原位置。</p>
                </div>
              </div>
              <button
                className="secondary-button"
                onClick={importDictionary}
                type="button"
              >
                <Icon name="folder" size={17} />
                选择词典文件
              </button>
              {externalDictionaries.length > 0 && (
                <div className="external-dictionary-list">
                  {externalDictionaries.map((dictionary, index) => (
                    <div className="external-dictionary" key={dictionary.id}>
                      <span className="dictionary-order">
                        {String(index + 1).padStart(2, "0")}
                      </span>
                      <div>
                        <strong>{dictionary.name}</strong>
                        <span>
                          {dictionary.format.toUpperCase()} ·{" "}
                          {dictionary.available ? "可用" : dictionary.error}
                        </span>
                      </div>
                      <button
                        aria-label={`上移 ${dictionary.name}`}
                        disabled={index === 0}
                        onClick={() => moveDictionary(index, -1)}
                        type="button"
                      >
                        ↑
                      </button>
                      <button
                        aria-label={`下移 ${dictionary.name}`}
                        disabled={index === externalDictionaries.length - 1}
                        onClick={() => moveDictionary(index, 1)}
                        type="button"
                      >
                        ↓
                      </button>
                      <button
                        aria-label={`移除 ${dictionary.name}`}
                        onClick={() => removeDictionary(dictionary.id)}
                        type="button"
                      >
                        <Icon name="close" size={14} />
                      </button>
                    </div>
                  ))}
                </div>
              )}
              <p className="muted-paragraph">
                外部词典按列表顺序优先展示。词典内容的授权由用户自行确认。
              </p>
            </section>
          </div>
        )}
        {section === "ai" && (
          <div className="settings-stack">
            <section className="settings-section">
              <div className="section-title">
                <div>
                  <h2>AI 服务</h2>
                  <p>键值仅保存到 Windows 凭据管理器，不写入配置文件。</p>
                </div>
                <span
                  className={`status-pill ${
                    aiKeyConfigured ? "is-ok" : "is-neutral"
                  }`}
                >
                  {aiKeyConfigured ? "已配置密钥" : "未配置"}
                </span>
              </div>
              <div className="provider-presets" aria-label="常用 AI 服务">
                <button
                  onClick={() => applyProviderPreset("deepseek")}
                  type="button"
                >
                  DeepSeek
                </button>
                <button
                  onClick={() => applyProviderPreset("openai")}
                  type="button"
                >
                  OpenAI
                </button>
                <button
                  onClick={() => applyProviderPreset("anthropic")}
                  type="button"
                >
                  Anthropic
                </button>
              </div>
              <label className="stacked-field">
                <span className="field-label">接口协议</span>
                <select
                  aria-label="AI 接口协议"
                  onChange={(event) =>
                    update(
                      "aiProtocol",
                      event.target.value as AppSettings["aiProtocol"],
                    )
                  }
                  value={settings.aiProtocol}
                >
                  <option value="auto">自动判断</option>
                  <option value="openai">OpenAI Chat Completions</option>
                  <option value="anthropic">Anthropic Messages</option>
                </select>
              </label>
              <label className="stacked-field">
                <span className="field-label">Base URL</span>
                <input
                  aria-label="AI Base URL"
                  onChange={(event) => update("aiBaseUrl", event.target.value)}
                  spellCheck={false}
                  type="url"
                  value={settings.aiBaseUrl}
                />
              </label>
              <label className="stacked-field">
                <span className="field-label">模型</span>
                <div className="model-row">
                  <input
                    aria-label="AI 模型名称"
                    list="eneye-model-options"
                    onChange={(event) => update("aiModel", event.target.value)}
                    placeholder="填写或从服务商读取模型 ID"
                    spellCheck={false}
                    type="text"
                    value={settings.aiModel}
                  />
                  <datalist id="eneye-model-options">
                    {modelOptions.map((model) => (
                      <option key={model} value={model} />
                    ))}
                  </datalist>
                  <button
                    className="secondary-button"
                    disabled={modelLoading}
                    onClick={refreshModelOptions}
                    type="button"
                  >
                    {modelLoading ? "读取中…" : "读取模型"}
                  </button>
                </div>
                {modelError && (
                  <span className="field-error" role="alert">
                    {modelError}
                  </span>
                )}
              </label>
              <label className="stacked-field">
                <span className="field-label">API Key</span>
                <div className="credential-row">
                  <input
                    aria-label="API Key"
                    autoComplete="off"
                    onChange={(event) => setAiKeyValue(event.target.value)}
                    placeholder={
                      aiKeyConfigured
                        ? "密钥已安全保存，输入新值可替换"
                        : "输入 API Key"
                    }
                    type="password"
                    value={aiKey}
                  />
                  <button
                    className="primary-button"
                    disabled={!aiKey.trim() || keyStatus === "saving"}
                    onClick={saveAiCredential}
                    type="button"
                  >
                    {keyStatus === "saving"
                      ? "保存中…"
                      : keyStatus === "saved"
                        ? "已保存"
                        : "保存密钥"}
                  </button>
                </div>
                {keyError && (
                  <span className="field-error" role="alert">
                    {keyError}
                  </span>
                )}
                {aiKeyConfigured && (
                  <button
                    className="text-button danger-text"
                    onClick={removeAiCredential}
                    type="button"
                  >
                    删除已保存的密钥
                  </button>
                )}
              </label>
            </section>
            <section className="settings-section">
              <div className="section-title">
                <div>
                  <h2>语言</h2>
                  <p>默认自动识别原文，并翻译为简体中文。</p>
                </div>
              </div>
              <div className="language-grid">
                <label className="stacked-field">
                  <span className="field-label">源语言</span>
                  <select
                    aria-label="AI 源语言"
                    onChange={(event) =>
                      update("sourceLanguage", event.target.value)
                    }
                    value={settings.sourceLanguage}
                  >
                    {SOURCE_LANGUAGES.map((language) => (
                      <option key={language.value} value={language.value}>
                        {language.label}
                      </option>
                    ))}
                  </select>
                </label>
                <label className="stacked-field">
                  <span className="field-label">目标语言</span>
                  <select
                    aria-label="AI 目标语言"
                    onChange={(event) =>
                      update("targetLanguage", event.target.value)
                    }
                    value={settings.targetLanguage}
                  >
                    {TARGET_LANGUAGES.map((language) => (
                      <option key={language} value={language}>
                        {language}
                      </option>
                    ))}
                  </select>
                </label>
              </div>
              <p className="muted-paragraph">
                AI 只接收拖选中的文字，不读取周围段落、截图或其他窗口内容。
              </p>
            </section>
          </div>
        )}
        {section === "history" && (
          <div className="settings-stack">
            <section className="settings-section history-section">
              <div className="history-toolbar">
                <label className="search-field">
                  <Icon name="search" size={17} />
                  <span className="sr-only">搜索查询历史</span>
                  <input
                    aria-label="搜索查询历史"
                    onChange={(event) => setHistoryQuery(event.target.value)}
                    placeholder="搜索单词或译文"
                    type="search"
                    value={historyQuery}
                  />
                </label>
                <button
                  className="text-button danger-text"
                  disabled={history.length === 0}
                  onClick={removeAllHistory}
                  type="button"
                >
                  清空历史
                </button>
              </div>
              {historyLoading ? (
                <div className="history-empty">正在读取本地历史…</div>
              ) : history.length === 0 ? (
                <div className="history-empty">
                  <Icon name="history" size={24} />
                  还没有查询记录
                </div>
              ) : (
                <div className="history-list">
                  {history.map((record) => (
                    <article className="history-item" key={record.id}>
                      <span className="history-kind">
                        {record.kind === "word" ? "词" : "句"}
                      </span>
                      <div>
                        <strong>{record.text}</strong>
                        {record.translation && <p>{record.translation}</p>}
                        <small>
                          {new Date(record.createdAt).toLocaleString("zh-CN")}
                          {record.dictionaries.length > 0 &&
                            ` · ${record.dictionaries.join(" / ")}`}
                        </small>
                      </div>
                      <button
                        aria-label={`删除 ${record.text}`}
                        className="history-delete"
                        onClick={() => removeHistoryRecord(record.id)}
                        type="button"
                      >
                        <Icon name="close" size={15} />
                      </button>
                    </article>
                  ))}
                </div>
              )}
            </section>
          </div>
        )}
        {section === "diagnostics" && (
          <div className="settings-stack">
            <section className="settings-section">
              <div className="section-title">
                <div>
                  <h2>运行状态</h2>
                  <p>所有检查均在本机完成，不上传诊断信息。</p>
                </div>
              </div>
              <DiagnosticRow
                detail={
                  dictionaryStatus.available
                    ? `${dictionaryStatus.entryCount.toLocaleString()} 个词条`
                    : dictionaryStatus.error || "数据库缺失"
                }
                label="ECDICT 词典"
                ok={dictionaryStatus.available}
              />
              <DiagnosticRow
                detail={
                  aiKeyConfigured ? "密钥已安全保存" : "配置密钥后可翻译选区"
                }
                label="AI 翻译"
                ok={aiKeyConfigured}
              />
              <DiagnosticRow
                detail={
                  ocrAvailable
                    ? "Windows 英文 OCR 已启用"
                    : "请在 Windows 语言设置中安装英文 OCR 可选功能"
                }
                label="本地 OCR"
                ok={ocrAvailable}
              />
              {!ocrAvailable && (
                <button
                  className="secondary-button"
                  onClick={() => void openUrl("ms-settings:regionlanguage")}
                  type="button"
                >
                  打开 Windows 语言设置
                </button>
              )}
              <DiagnosticRow
                detail="Windows 10 22H2 / Windows 11 x64"
                label="系统接口"
                ok
              />
            </section>
          </div>
        )}
      </section>
    </main>
  );
}

function DiagnosticRow({
  label,
  detail,
  ok,
}: {
  label: string;
  detail: string;
  ok: boolean;
}) {
  return (
    <div className="diagnostic-row">
      <span className={`status-dot ${ok ? "is-ok" : "is-neutral"}`} />
      <div>
        <strong>{label}</strong>
        <span>{detail}</span>
      </div>
      <span className={ok ? "diagnostic-ok" : "diagnostic-neutral"}>
        {ok ? "正常" : "待配置"}
      </span>
    </div>
  );
}

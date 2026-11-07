import {
  useEffect,
  useMemo,
  useRef,
  useState,
  type CSSProperties,
} from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { openUrl } from "@tauri-apps/plugin-opener";
import { FONT_OPTIONS } from "../data/sample";
import {
  addExternalDictionary,
  createNotebook,
  deleteAiKey,
  deleteNotebook,
  detectAgentConnections,
  getCaptureDiagnostics,
  getDictionaryRegistryError,
  listAiModels,
  listNotebooks,
  listNotebookWords,
  relinkExternalDictionary,
  reloadExternalDictionary,
  removeExternalDictionary,
  recordRecognitionCorrection,
  removeNotebookWord,
  reorderExternalDictionaries,
  setAiKey,
} from "../lib/api";
import { isTauriRuntime } from "../lib/tauri";
import type {
  AgentConnectionStatus,
  AppSettings,
  AiEffort,
  AiRoles,
  DictionaryStatus,
  ExternalDictionary,
  Notebook,
  NotebookWord,
  WordProbeReport,
} from "../types";
import { DictionarySearchPanel } from "./DictionarySearchPanel";
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
  { id: "history", label: "生词本", icon: "book" },
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
  agentConnections: AgentConnectionStatus[];
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
  agentConnections,
  aiKeyConfigured,
  ocrAvailable,
  onAiKeyConfiguredChange,
  onExternalDictionariesChange,
}: SettingsWindowProps) {
  const [section, setSection] = useState<Section>("general");
  const [sampleActive, setSampleActive] = useState(false);
  const sampleShowTimer = useRef<number | undefined>(undefined);
  const sampleHideTimer = useRef<number | undefined>(undefined);
  const [aiKey, setAiKeyValue] = useState("");
  const [keyStatus, setKeyStatus] = useState<"idle" | "saving" | "saved">(
    "idle",
  );
  const [keyError, setKeyError] = useState("");
  const [modelOptions, setModelOptions] = useState<string[]>([]);
  const [modelLoading, setModelLoading] = useState(false);
  const [modelError, setModelError] = useState("");
  const [apiEditorOpen, setApiEditorOpen] = useState(false);
  const [apiTestStatus, setApiTestStatus] = useState("");
  const [notebooks, setNotebooks] = useState<Notebook[]>([]);
  const [selectedNotebookId, setSelectedNotebookId] = useState<number | null>(
    null,
  );
  const [notebookWords, setNotebookWords] = useState<NotebookWord[]>([]);
  const [notebookQuery, setNotebookQuery] = useState("");
  const [notebookLoading, setNotebookLoading] = useState(false);
  const [notebookError, setNotebookError] = useState("");
  const [dictionaryBusy, setDictionaryBusy] = useState(false);
  const [dictionaryError, setDictionaryError] = useState("");
  const [dictionaryNotice, setDictionaryNotice] = useState("");
  const [dictionaryRegistryError, setDictionaryRegistryError] = useState("");
  const [creatingNotebook, setCreatingNotebook] = useState(false);
  const [newNotebookName, setNewNotebookName] = useState("");
  const [notebookSaving, setNotebookSaving] = useState(false);
  const [probeReport, setProbeReport] = useState<WordProbeReport | null>(null);
  const [correctionOpen, setCorrectionOpen] = useState(false);
  const [correctionWord, setCorrectionWord] = useState("");
  const [correctionStatus, setCorrectionStatus] = useState("");
  const [liveAgentConnections, setLiveAgentConnections] =
    useState<AgentConnectionStatus[]>(agentConnections);
  const [agentDetectionLoading, setAgentDetectionLoading] = useState(false);
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
  const roleModelOptions = useMemo(
    () =>
      Array.from(
        new Set(
          [
            ...modelOptions,
            settings.aiModel,
            settings.aiRoles.chat.model,
            settings.aiRoles.translation.model,
            settings.aiRoles.image.model,
          ].filter(Boolean),
        ),
      ),
    [modelOptions, settings.aiModel, settings.aiRoles],
  );
  const activeNotebook =
    notebooks.find((notebook) => notebook.id === selectedNotebookId) ?? null;

  const update = <K extends keyof AppSettings>(key: K, value: AppSettings[K]) =>
    onSettingsChange({ ...settings, [key]: value });

  useEffect(() => {
    if (!isTauriRuntime()) return;
    getDictionaryRegistryError()
      .then((error) => {
        if (error) setDictionaryRegistryError(error);
      })
      .catch(() => undefined);
  }, []);

  function showSamplePreview() {
    window.clearTimeout(sampleHideTimer.current);
    window.clearTimeout(sampleShowTimer.current);
    if (sampleActive) return;
    sampleShowTimer.current = window.setTimeout(
      () => setSampleActive(true),
      settings.lookupDelayMs,
    );
  }

  function hideSamplePreview() {
    window.clearTimeout(sampleShowTimer.current);
    window.clearTimeout(sampleHideTimer.current);
    sampleHideTimer.current = window.setTimeout(
      () => setSampleActive(false),
      settings.wordCardLingerMs,
    );
  }

  function showSamplePreviewNow() {
    window.clearTimeout(sampleShowTimer.current);
    window.clearTimeout(sampleHideTimer.current);
    setSampleActive(true);
  }

  useEffect(() => {
    return () => {
      window.clearTimeout(sampleShowTimer.current);
      window.clearTimeout(sampleHideTimer.current);
    };
  }, []);

  function updateRole(
    role: keyof AiRoles,
    patch: Partial<AiRoles[keyof AiRoles]>,
  ) {
    const nextRoles = {
      ...settings.aiRoles,
      [role]: { ...settings.aiRoles[role], ...patch },
    };
    onSettingsChange({
      ...settings,
      aiRoles: nextRoles,
      ...(role === "translation" && patch.model !== undefined
        ? { aiModel: patch.model }
        : {}),
    });
  }

  useEffect(() => {
    setLiveAgentConnections(agentConnections);
  }, [agentConnections]);

  useEffect(() => {
    if (section !== "history") return;
    let active = true;
    setNotebookLoading(true);
    listNotebooks()
      .then((items) => {
        if (!active) return;
        setNotebooks(items);
        setSelectedNotebookId((current) =>
          current != null && items.some((item) => item.id === current)
            ? current
            : (items[0]?.id ?? null),
        );
      })
      .catch((error) => {
        if (active) setNotebookError(String(error));
      })
      .finally(() => {
        if (active) setNotebookLoading(false);
      });
    return () => {
      active = false;
    };
  }, [section]);

  useEffect(() => {
    if (section !== "history" || selectedNotebookId == null) {
      setNotebookWords([]);
      return;
    }
    let active = true;
    setNotebookLoading(true);
    listNotebookWords(selectedNotebookId, notebookQuery)
      .then((words) => {
        if (active) setNotebookWords(words);
      })
      .catch((error) => {
        if (active) setNotebookError(String(error));
      })
      .finally(() => {
        if (active) setNotebookLoading(false);
      });
    return () => {
      active = false;
    };
  }, [notebookQuery, section, selectedNotebookId]);

  useEffect(() => {
    if (section !== "diagnostics") return;
    let active = true;
    getCaptureDiagnostics().then((report) => {
      if (active) setProbeReport(report);
    });
    return () => {
      active = false;
    };
  }, [section]);

  useEffect(() => {
    if (!apiEditorOpen) return;
    function closeOnEscape(event: KeyboardEvent) {
      if (event.key === "Escape") setApiEditorOpen(false);
    }
    window.addEventListener("keydown", closeOnEscape);
    return () => window.removeEventListener("keydown", closeOnEscape);
  }, [apiEditorOpen]);

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
      aiModel: "",
      aiRoles: {
        chat: { ...settings.aiRoles.chat, model: "" },
        translation: { ...settings.aiRoles.translation, model: "" },
        image: { ...settings.aiRoles.image, model: "" },
      },
    });
  }

  async function refreshModelOptions() {
    setModelError("");
    setModelLoading(true);
    try {
      const models = await listAiModels(settings, aiKey);
      setModelOptions(models);
      if (models.length > 0) {
        const fallback = models[0];
        const nextRoles = { ...settings.aiRoles };
        for (const role of ["chat", "translation", "image"] as const) {
          if (!nextRoles[role].model) {
            nextRoles[role] = { ...nextRoles[role], model: fallback };
          }
        }
        onSettingsChange({
          ...settings,
          aiModel: nextRoles.translation.model,
          aiRoles: nextRoles,
        });
      }
      setApiTestStatus(`连接成功，读取到 ${models.length} 个模型`);
    } catch (error) {
      setModelError(String(error));
      setApiTestStatus("连接失败");
    } finally {
      setModelLoading(false);
    }
  }

  async function refreshAgentStatus() {
    setAgentDetectionLoading(true);
    try {
      setLiveAgentConnections(await detectAgentConnections());
    } finally {
      setAgentDetectionLoading(false);
    }
  }

  async function refreshNotebooks(selectId?: number | null) {
    setNotebookLoading(true);
    try {
      const items = await listNotebooks();
      setNotebooks(items);
      setSelectedNotebookId((current) => {
        const candidate = selectId ?? current;
        return candidate != null && items.some((item) => item.id === candidate)
          ? candidate
          : (items[0]?.id ?? null);
      });
    } catch (error) {
      setNotebookError(String(error));
    } finally {
      setNotebookLoading(false);
    }
  }

  async function submitNewNotebook() {
    const name = newNotebookName.trim();
    if (!name || notebookSaving) return;
    setNotebookSaving(true);
    setNotebookError("");
    try {
      const notebook = await createNotebook(name);
      setNewNotebookName("");
      setCreatingNotebook(false);
      await refreshNotebooks(notebook.id);
    } catch (error) {
      setNotebookError(String(error));
    } finally {
      setNotebookSaving(false);
    }
  }

  async function removeCurrentNotebook() {
    if (selectedNotebookId == null) return;
    if (!window.confirm("删除这个本子以及里面的所有单词？")) return;
    setNotebookError("");
    try {
      await deleteNotebook(selectedNotebookId);
      await refreshNotebooks(null);
    } catch (error) {
      setNotebookError(String(error));
    }
  }

  async function removeWordFromNotebook(id: number) {
    setNotebookError("");
    try {
      await removeNotebookWord(id);
      setNotebookWords((words) => words.filter((word) => word.id !== id));
      await refreshNotebooks(selectedNotebookId);
    } catch (error) {
      setNotebookError(String(error));
    }
  }

  async function importDictionary() {
    if (!isTauriRuntime() || dictionaryBusy) return;
    setDictionaryError("");
    setDictionaryNotice("");
    try {
      const selected = await open({
        multiple: false,
        directory: false,
        filters: [
          {
            name: "词典文件",
            extensions: ["csv", "mdx", "ifo", "idx", "dict", "gz", "syn"],
          },
        ],
      });
      if (typeof selected !== "string") return;
      setDictionaryBusy(true);
      const dictionary = await addExternalDictionary(selected);
      onExternalDictionariesChange([...externalDictionaries, dictionary]);
      setDictionaryNotice(`已添加 ${dictionary.name}`);
    } catch (error) {
      setDictionaryError(String(error));
    } finally {
      setDictionaryBusy(false);
    }
  }

  async function relinkDictionary(id: string) {
    if (!isTauriRuntime() || dictionaryBusy) return;
    setDictionaryError("");
    setDictionaryNotice("");
    try {
      const selected = await open({
        multiple: false,
        directory: false,
        filters: [
          {
            name: "词典文件",
            extensions: ["csv", "mdx", "ifo", "idx", "dict", "gz", "syn"],
          },
        ],
      });
      if (typeof selected !== "string") return;
      setDictionaryBusy(true);
      const updated = await relinkExternalDictionary(id, selected);
      onExternalDictionariesChange(
        externalDictionaries.map((dictionary) =>
          dictionary.id === id ? updated : dictionary,
        ),
      );
      setDictionaryNotice(`已重新定位 ${updated.name}`);
    } catch (error) {
      setDictionaryError(String(error));
    } finally {
      setDictionaryBusy(false);
    }
  }

  async function reloadDictionary(id: string) {
    if (!isTauriRuntime() || dictionaryBusy) return;
    setDictionaryError("");
    setDictionaryNotice("");
    try {
      setDictionaryBusy(true);
      const updated = await reloadExternalDictionary(id);
      onExternalDictionariesChange(
        externalDictionaries.map((dictionary) =>
          dictionary.id === id ? updated : dictionary,
        ),
      );
      setDictionaryNotice(`已重新加载 ${updated.name}`);
    } catch (error) {
      setDictionaryError(String(error));
    } finally {
      setDictionaryBusy(false);
    }
  }

  async function removeDictionary(id: string, name: string) {
    if (dictionaryBusy) return;
    if (!window.confirm(`从列表中移除“${name}”？词典文件本身不会被删除。`)) {
      return;
    }
    setDictionaryError("");
    setDictionaryNotice("");
    try {
      setDictionaryBusy(true);
      await removeExternalDictionary(id);
      onExternalDictionariesChange(
        externalDictionaries
          .filter((dictionary) => dictionary.id !== id)
          .map((dictionary, index) => ({ ...dictionary, priority: index })),
      );
      setDictionaryNotice(`已移除 ${name}`);
    } catch (error) {
      setDictionaryError(String(error));
    } finally {
      setDictionaryBusy(false);
    }
  }

  async function moveDictionary(index: number, direction: -1 | 1) {
    if (dictionaryBusy) return;
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
    setDictionaryError("");
    setDictionaryNotice("");
    try {
      setDictionaryBusy(true);
      await reorderExternalDictionaries(
        withPriorities.map((dictionary) => dictionary.id),
      );
      onExternalDictionariesChange(withPriorities);
    } catch (error) {
      setDictionaryError(String(error));
    } finally {
      setDictionaryBusy(false);
    }
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
          <kbd>Ctrl Alt E</kbd>
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
                  min="100"
                  onChange={(event) =>
                    update("lookupDelayMs", Number(event.target.value))
                  }
                  step="10"
                  type="range"
                  value={settings.lookupDelayMs}
                />
                <span className="range-labels">
                  <span>灵敏 100ms</span>
                  <span>稳定 500ms</span>
                </span>
              </label>
              <label className="range-field range-field-secondary">
                <span>移开后保留时间</span>
                <input
                  aria-label="移开后保留时间"
                  max="5000"
                  min="0"
                  onChange={(event) =>
                    update("wordCardLingerMs", Number(event.target.value))
                  }
                  step="100"
                  type="range"
                  value={settings.wordCardLingerMs}
                />
                <span className="range-labels">
                  <span>立即 0ms</span>
                  <span>
                    当前{" "}
                    {settings.wordCardLingerMs === 0
                      ? "立即"
                      : `${settings.wordCardLingerMs}ms`}
                  </span>
                  <span>持久 5000ms</span>
                </span>
              </label>
              <label className="range-field range-field-secondary">
                <span>悬浮窗水平偏移 · {settings.overlayOffsetXPx}px</span>
                <input
                  aria-label="悬浮窗水平偏移"
                  max="240"
                  min="0"
                  onChange={(event) =>
                    update("overlayOffsetXPx", Number(event.target.value))
                  }
                  step="2"
                  type="range"
                  value={settings.overlayOffsetXPx}
                />
                <span className="range-labels">
                  <span>贴近 0px</span>
                  <span>远离 240px</span>
                </span>
              </label>
              <label className="range-field range-field-secondary">
                <span>悬浮窗垂直偏移 · {settings.overlayOffsetYPx}px</span>
                <input
                  aria-label="悬浮窗垂直偏移"
                  max="240"
                  min="0"
                  onChange={(event) =>
                    update("overlayOffsetYPx", Number(event.target.value))
                  }
                  step="2"
                  type="range"
                  value={settings.overlayOffsetYPx}
                />
                <span className="range-labels">
                  <span>贴近 0px</span>
                  <span>远离 240px</span>
                </span>
              </label>
              <button
                className={`hover-preview ${sampleActive ? "is-active" : ""}`}
                onClick={showSamplePreviewNow}
                onMouseEnter={showSamplePreview}
                onMouseLeave={hideSamplePreview}
                style={
                  {
                    "--preview-offset-x": `${settings.overlayOffsetXPx}px`,
                    "--preview-offset-y": `${settings.overlayOffsetYPx}px`,
                  } as CSSProperties
                }
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
            <DictionarySearchPanel />
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
                disabled={dictionaryBusy}
                onClick={importDictionary}
                type="button"
              >
                <Icon name="folder" size={17} />
                {dictionaryBusy ? "正在读取词典…" : "选择词典文件"}
              </button>
              {dictionaryRegistryError && (
                <p className="inline-error" role="alert">
                  词典列表读取失败，原文件已备份为 dictionaries.json.corrupt。
                  {dictionaryRegistryError}
                </p>
              )}
              {dictionaryError && (
                <p className="inline-error" role="alert">
                  {dictionaryError}
                </p>
              )}
              {dictionaryNotice && (
                <p className="inline-notice" role="status">
                  {dictionaryNotice}
                </p>
              )}
              {externalDictionaries.length > 0 && (
                <div className="external-dictionary-list">
                  {externalDictionaries.map((dictionary, index) => (
                    <div className="external-dictionary" key={dictionary.id}>
                      <span className="dictionary-order">
                        {String(index + 1).padStart(2, "0")}
                      </span>
                      <div className="dictionary-copy">
                        <strong>{dictionary.name}</strong>
                        <span>
                          {dictionary.format.toUpperCase()} ·{" "}
                          {dictionary.available
                            ? "可用"
                            : dictionary.error || "不可用"}
                        </span>
                        <span
                          className="dictionary-path"
                          title={dictionary.path}
                        >
                          {dictionary.path}
                        </span>
                      </div>
                      <button
                        aria-label={`上移 ${dictionary.name}`}
                        disabled={index === 0 || dictionaryBusy}
                        onClick={() => moveDictionary(index, -1)}
                        type="button"
                      >
                        ↑
                      </button>
                      <button
                        aria-label={`下移 ${dictionary.name}`}
                        disabled={
                          index === externalDictionaries.length - 1 ||
                          dictionaryBusy
                        }
                        onClick={() => moveDictionary(index, 1)}
                        type="button"
                      >
                        ↓
                      </button>
                      <button
                        aria-label={`重新定位 ${dictionary.name}`}
                        disabled={dictionaryBusy}
                        onClick={() => relinkDictionary(dictionary.id)}
                        title="重新定位"
                        type="button"
                      >
                        定位
                      </button>
                      <button
                        aria-label={`重新加载 ${dictionary.name}`}
                        disabled={dictionaryBusy}
                        onClick={() => reloadDictionary(dictionary.id)}
                        title="重新加载"
                        type="button"
                      >
                        刷新
                      </button>
                      <button
                        aria-label={`移除 ${dictionary.name}`}
                        disabled={dictionaryBusy}
                        onClick={() =>
                          removeDictionary(dictionary.id, dictionary.name)
                        }
                        type="button"
                      >
                        <Icon name="close" size={14} />
                      </button>
                    </div>
                  ))}
                </div>
              )}
              <p className="muted-paragraph">
                支持 CSV、MDX（可带同名 MDD）与 StarDict（选择 .ifo）。
                词典文件保留原位置；移动文件后可点“定位”重新关联。外部词典按列表顺序优先展示，词典内容的授权由用户自行确认。
              </p>
            </section>
          </div>
        )}
        {section === "ai" && (
          <div className="settings-stack">
            <section className="settings-section">
              <div className="setting-row">
                <span>
                  <span className="field-label">启用 AI 翻译</span>
                  <span className="field-help">
                    关闭后不响应拖选翻译、Ctrl+Alt+T 和卡片里的 AI
                    解释；词典查词不受影响
                  </span>
                </span>
                <Toggle
                  checked={settings.aiTranslationEnabled}
                  label="启用 AI 翻译"
                  onChange={(value) => update("aiTranslationEnabled", value)}
                />
              </div>
            </section>

            <section className="settings-section agent-settings-section">
              <div className="section-title">
                <div>
                  <h2>Agent 连接</h2>
                  <p>选择 Eneye 可使用的 AI 服务。</p>
                </div>
                <button
                  aria-label="刷新连接状态"
                  className="icon-button agent-refresh"
                  disabled={agentDetectionLoading}
                  onClick={() => void refreshAgentStatus()}
                  type="button"
                >
                  <Icon name="pulse" />
                </button>
              </div>
              <div className="agent-connection-grid">
                {(
                  [
                    ["codex", "Codex", "C", ""],
                    ["claude-code", "Claude Code", "C", "is-claude"],
                    ["kimi-code", "Kimi Code", "K", "is-kimi"],
                  ] as const
                ).map(([id, name, logo, logoClass]) => {
                  const status = liveAgentConnections.find(
                    (connection) => connection.id === id,
                  );
                  return (
                    <article
                      className={`agent-connection-card ${
                        status?.available ? "is-available" : "is-unavailable"
                      }`}
                      key={id}
                    >
                      <span className={`agent-logo ${logoClass}`}>{logo}</span>
                      <div>
                        <strong>{name}</strong>
                        <span>
                          {status?.detail ??
                            (agentDetectionLoading ? "正在检测…" : "尚未检测")}
                        </span>
                      </div>
                      <span
                        className={`agent-state ${
                          status?.available ? "is-ok" : ""
                        }`}
                      >
                        {status?.available ? "已检测" : "未检测到"}
                      </span>
                    </article>
                  );
                })}
                <button
                  className="agent-connection-card is-api"
                  onClick={() => setApiEditorOpen(true)}
                  type="button"
                >
                  <span className="agent-logo is-api">⌁</span>
                  <div>
                    <strong>{settings.aiConnectionName || "API"}</strong>
                    <span className="agent-capabilities">
                      <small className={aiKeyConfigured ? "is-ok" : ""}>
                        ✓ 文本推理
                      </small>
                      <small className="is-muted">图片理解（本版未使用）</small>
                    </span>
                  </div>
                  <Icon name="chevron" size={17} />
                </button>
              </div>
            </section>

            <section className="settings-section agent-models-section">
              <div className="section-title">
                <div>
                  <h2>模型与推理</h2>
                  <p>Eneye 当前使用“翻译”角色。</p>
                </div>
                <button
                  className="text-button"
                  disabled={modelLoading}
                  onClick={refreshModelOptions}
                  type="button"
                >
                  {modelLoading ? "读取中…" : "刷新模型"}
                </button>
              </div>
              {(["chat", "translation", "image"] as const).map((role) => {
                const roleLabels = {
                  chat: "问答",
                  translation: "翻译",
                  image: "图片",
                };
                return (
                  <div className="agent-role-row" key={role}>
                    <strong>{roleLabels[role]}</strong>
                    <input
                      aria-label={`${roleLabels[role]}模型`}
                      list={`model-options-${role}`}
                      onChange={(event) =>
                        updateRole(role, { model: event.target.value })
                      }
                      placeholder="选择或输入模型"
                      value={settings.aiRoles[role].model}
                    />
                    <datalist id={`model-options-${role}`}>
                      {roleModelOptions.map((model) => (
                        <option key={model} value={model} />
                      ))}
                    </datalist>
                    <select
                      aria-label={`${roleLabels[role]}推理强度`}
                      onChange={(event) =>
                        updateRole(role, {
                          effort: event.target.value as AiEffort,
                        })
                      }
                      value={settings.aiRoles[role].effort}
                    >
                      <option value="low">Low</option>
                      <option value="medium">Medium</option>
                      <option value="high">High</option>
                    </select>
                  </div>
                );
              })}
              <p className="agent-rule-note">
                按官方模型规则发送。medium → high
              </p>
              {!settings.aiRoles.translation.model.trim() &&
                !settings.aiModel.trim() && (
                  <span className="field-error" role="status">
                    尚未选择“翻译”模型，翻译和解释都无法启动。
                  </span>
                )}
              {modelError && (
                <span className="field-error" role="alert">
                  {modelError}
                </span>
              )}
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

            {apiEditorOpen && (
              <div
                className="modal-backdrop"
                onMouseDown={(event) => {
                  if (event.target === event.currentTarget) {
                    setApiEditorOpen(false);
                  }
                }}
              >
                <section
                  aria-label="API 设置"
                  aria-modal="true"
                  className="api-editor"
                  role="dialog"
                >
                  <header className="api-editor-header">
                    <div>
                      <p className="eyebrow">API CONNECTION</p>
                      <h2>编辑 API</h2>
                    </div>
                    <button
                      aria-label="关闭 API 设置"
                      className="icon-button"
                      onClick={() => setApiEditorOpen(false)}
                      type="button"
                    >
                      <Icon name="close" />
                    </button>
                  </header>
                  <div className="api-editor-body">
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
                      <span className="field-label">连接名称</span>
                      <input
                        aria-label="AI 连接名称"
                        onChange={(event) =>
                          update("aiConnectionName", event.target.value)
                        }
                        type="text"
                        value={settings.aiConnectionName}
                      />
                    </label>
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
                      <span className="field-label">API 地址</span>
                      <input
                        aria-label="AI Base URL"
                        onChange={(event) =>
                          update("aiBaseUrl", event.target.value)
                        }
                        spellCheck={false}
                        type="url"
                        value={settings.aiBaseUrl}
                      />
                    </label>
                    <label className="stacked-field">
                      <span className="field-label">API Key</span>
                      <div className="credential-row">
                        <input
                          aria-label="API Key"
                          autoComplete="off"
                          onChange={(event) =>
                            setAiKeyValue(event.target.value)
                          }
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
                    {apiTestStatus && (
                      <p
                        className={
                          apiTestStatus === "连接失败"
                            ? "api-test-status is-error"
                            : "api-test-status"
                        }
                      >
                        {apiTestStatus}
                      </p>
                    )}
                  </div>
                  <footer className="api-editor-footer">
                    <button
                      className="secondary-button"
                      disabled={modelLoading}
                      onClick={refreshModelOptions}
                      type="button"
                    >
                      {modelLoading ? "测试中…" : "测试连接并读取模型"}
                    </button>
                    <button
                      className="primary-button"
                      onClick={() => setApiEditorOpen(false)}
                      type="button"
                    >
                      完成
                    </button>
                  </footer>
                </section>
              </div>
            )}
          </div>
        )}
        {section === "history" && (
          <div className="settings-stack">
            <section className="settings-section notebook-section">
              <div className="notebook-layout">
                <aside className="notebook-sidebar">
                  <div className="notebook-sidebar-head">
                    <span>我的本子</span>
                    <button
                      aria-label="新建本子"
                      className="icon-button"
                      onClick={() => {
                        setCreatingNotebook(true);
                        setNotebookError("");
                      }}
                      type="button"
                    >
                      <Icon name="plus" size={16} />
                    </button>
                  </div>
                  {creatingNotebook && (
                    <form
                      className="notebook-create-form"
                      onSubmit={(event) => {
                        event.preventDefault();
                        void submitNewNotebook();
                      }}
                    >
                      <input
                        aria-label="新本子名称"
                        autoFocus
                        maxLength={24}
                        onChange={(event) =>
                          setNewNotebookName(event.target.value)
                        }
                        placeholder="例如：生词本"
                        value={newNotebookName}
                      />
                      <button
                        className="primary-button"
                        disabled={notebookSaving || !newNotebookName.trim()}
                        type="submit"
                      >
                        创建
                      </button>
                      <button
                        className="text-button"
                        onClick={() => {
                          setCreatingNotebook(false);
                          setNewNotebookName("");
                        }}
                        type="button"
                      >
                        取消
                      </button>
                    </form>
                  )}
                  <div className="notebook-list">
                    {notebooks.map((notebook) => (
                      <div
                        className={`notebook-row ${
                          notebook.id === selectedNotebookId ? "is-active" : ""
                        }`}
                        key={notebook.id}
                      >
                        <button
                          className="notebook-select"
                          onClick={() => setSelectedNotebookId(notebook.id)}
                          type="button"
                        >
                          <Icon name="book" size={15} />
                          <span>{notebook.name}</span>
                          <small>{notebook.wordCount}</small>
                        </button>
                      </div>
                    ))}
                  </div>
                  {notebooks.length === 0 && (
                    <p className="notebook-hint">
                      还没有本子。点右上角的 + 创建一个。
                    </p>
                  )}
                </aside>

                <div className="notebook-main">
                  <div className="notebook-main-head">
                    <div>
                      <h3>{activeNotebook?.name ?? "生词本"}</h3>
                      <p>
                        {activeNotebook
                          ? `${activeNotebook.wordCount} 个单词`
                          : "选择一个本子，或先创建一个。"}
                      </p>
                    </div>
                    {activeNotebook && (
                      <button
                        className="text-button danger-text"
                        onClick={() => void removeCurrentNotebook()}
                        type="button"
                      >
                        删除本子
                      </button>
                    )}
                  </div>
                  <label className="search-field">
                    <Icon name="search" size={17} />
                    <span className="sr-only">搜索本子里的单词</span>
                    <input
                      aria-label="搜索本子里的单词"
                      disabled={!activeNotebook}
                      onChange={(event) => setNotebookQuery(event.target.value)}
                      placeholder="搜索单词或释义"
                      type="search"
                      value={notebookQuery}
                    />
                  </label>
                  {notebookError && (
                    <p className="inline-error" role="alert">
                      <Icon name="alert" size={16} />
                      {notebookError}
                    </p>
                  )}
                  {notebookLoading ? (
                    <div className="history-empty">正在读取本子…</div>
                  ) : notebookWords.length === 0 ? (
                    <div className="history-empty">
                      <Icon name="book" size={24} />
                      {notebookQuery
                        ? "没有匹配的单词"
                        : "这个本子还是空的。把鼠标停在单词上，点悬浮窗里的收藏按钮就能放进来。"}
                    </div>
                  ) : (
                    <div className="history-list">
                      {notebookWords.map((word) => (
                        <article className="history-item" key={word.id}>
                          <span className="history-kind">词</span>
                          <div>
                            <strong>{word.word}</strong>
                            {word.phonetic && <small>{word.phonetic}</small>}
                            {word.definition && <p>{word.definition}</p>}
                            <small>
                              {new Date(word.createdAt).toLocaleString("zh-CN")}
                            </small>
                          </div>
                          <button
                            aria-label={`从本子移除 ${word.word}`}
                            className="history-delete"
                            onClick={() => void removeWordFromNotebook(word.id)}
                            type="button"
                          >
                            <Icon name="close" size={15} />
                          </button>
                        </article>
                      ))}
                    </div>
                  )}
                </div>
              </div>
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
                    ? "Windows 本地 OCR 已启用（优先英文，缺失时使用已安装语言）"
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
              <div className="probe-report">
                <div className="probe-report-heading">
                  <div>
                    <strong>最近一次取词探针</strong>
                    <span>
                      {probeReport?.word
                        ? `取词 ${probeReport.word} · ${probeReport.source}`
                        : "尚未命中单词"}
                      {probeReport?.confidence != null
                        ? ` · 置信度 ${Math.round(probeReport.confidence * 100)}%`
                        : ""}
                      {probeReport ? ` · ${probeReport.totalMs}ms` : ""}
                    </span>
                  </div>
                  <button
                    className="text-button"
                    onClick={() =>
                      void getCaptureDiagnostics().then(setProbeReport)
                    }
                    type="button"
                  >
                    刷新
                  </button>
                </div>
                {(probeReport?.stages ?? []).map((stage) => (
                  <div className="probe-stage" key={stage.provider}>
                    <span
                      className={`status-dot ${
                        stage.available ? "is-ok" : "is-neutral"
                      }`}
                    />
                    <strong>{stage.provider.toUpperCase()}</strong>
                    <span>{stage.detail}</span>
                    <time>{stage.durationMs}ms</time>
                  </div>
                ))}
                {probeReport?.reason && (
                  <p className="probe-reason">判断依据：{probeReport.reason}</p>
                )}
                {(probeReport?.candidates ?? []).length > 0 && (
                  <div className="probe-candidates">
                    <strong>候选排序</strong>
                    {probeReport?.candidates
                      ?.slice(0, 6)
                      .map((candidate, index) => (
                        <div
                          className={`probe-candidate ${
                            candidate.text === probeReport.word
                              ? "is-chosen"
                              : ""
                          }`}
                          key={`${candidate.text}-${candidate.provider}-${index}`}
                        >
                          <span className="probe-candidate-word">
                            {candidate.text}
                          </span>
                          <span className="probe-candidate-score">
                            {Math.round(candidate.confidence * 100)}%
                          </span>
                          <span className="probe-candidate-reason">
                            {candidate.reason}
                          </span>
                        </div>
                      ))}
                  </div>
                )}
                <div className="probe-correction">
                  <button
                    className="text-button"
                    onClick={() => {
                      setCorrectionOpen((open) => !open);
                      setCorrectionWord(probeReport?.word ?? "");
                      setCorrectionStatus("");
                    }}
                    type="button"
                  >
                    这次识别错了
                  </button>
                  {correctionOpen && (
                    <form
                      className="probe-correction-form"
                      onSubmit={(event) => {
                        event.preventDefault();
                        const corrected = correctionWord.trim();
                        if (!corrected) {
                          setCorrectionStatus("请先填写正确的词");
                          return;
                        }
                        setCorrectionStatus("正在保存…");
                        void recordRecognitionCorrection({
                          observed: probeReport?.word ?? "",
                          corrected,
                          source: probeReport?.source ?? null,
                          confidence: probeReport?.confidence ?? null,
                          reason: probeReport?.reason ?? null,
                          candidates: probeReport?.candidates ?? [],
                        })
                          .then(() => {
                            setCorrectionStatus("已记录到本机诊断文件");
                            setCorrectionOpen(false);
                          })
                          .catch((error: unknown) =>
                            setCorrectionStatus(
                              error instanceof Error
                                ? error.message
                                : String(error),
                            ),
                          );
                      }}
                    >
                      <input
                        aria-label="正确的词"
                        autoFocus
                        onChange={(event) =>
                          setCorrectionWord(event.target.value)
                        }
                        placeholder="填写正确的词"
                        value={correctionWord}
                      />
                      <button className="secondary-button" type="submit">
                        保存
                      </button>
                    </form>
                  )}
                  {correctionStatus && (
                    <span className="probe-correction-status">
                      {correctionStatus}
                    </span>
                  )}
                </div>
              </div>
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

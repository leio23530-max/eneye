import { useEffect, useRef, useState } from "react";
import { OverlayWindow } from "./components/OverlayWindow";
import { SettingsWindow } from "./components/SettingsWindow";
import { DEFAULT_SETTINGS, FONT_OPTIONS } from "./data/sample";
import { loadBootstrap, saveSettings } from "./lib/api";
import { getWindowKind, isTauriRuntime } from "./lib/tauri";
import type {
  AgentConnectionStatus,
  AppSettings,
  BootstrapData,
  DictionaryStatus,
  ExternalDictionary,
} from "./types";

const PREVIEW_DICTIONARY: DictionaryStatus = {
  available: true,
  name: "ECDICT",
  entryCount: 770_612,
  path: "浏览器预览数据",
};

export default function App() {
  const [settings, setSettings] = useState<AppSettings>(DEFAULT_SETTINGS);
  const [fonts, setFonts] = useState(FONT_OPTIONS.map((font) => font.family));
  const [dictionaryStatus, setDictionaryStatus] =
    useState<DictionaryStatus>(PREVIEW_DICTIONARY);
  const [aiKeyConfigured, setAiKeyConfigured] = useState(false);
  const [ocrAvailable, setOcrAvailable] = useState(true);
  const [externalDictionaries, setExternalDictionaries] = useState<
    ExternalDictionary[]
  >([]);
  const [agentConnections, setAgentConnections] = useState<
    AgentConnectionStatus[]
  >([]);
  const [loaded, setLoaded] = useState(false);
  const saveTimer = useRef<number | undefined>(undefined);
  const readyToSave = useRef(false);
  const windowKind = getWindowKind();

  useEffect(() => {
    document.documentElement.dataset.window = windowKind;
    return () => {
      delete document.documentElement.dataset.window;
    };
  }, [windowKind]);

  useEffect(() => {
    let active = true;
    loadBootstrap()
      .then((bootstrap: BootstrapData) => {
        if (!active) return;
        setSettings(bootstrap.settings);
        setFonts(bootstrap.fonts);
        setDictionaryStatus(bootstrap.dictionaryStatus);
        setExternalDictionaries(bootstrap.externalDictionaries);
        setAgentConnections(bootstrap.agentConnections);
        setAiKeyConfigured(bootstrap.aiKeyConfigured);
        setOcrAvailable(bootstrap.ocrAvailable);
        readyToSave.current = true;
        setLoaded(true);
      })
      .catch((error) => {
        console.error("Failed to load Eneye bootstrap", error);
        setLoaded(true);
      });
    return () => {
      active = false;
    };
  }, []);

  useEffect(() => {
    if (!readyToSave.current || !loaded) return;
    window.clearTimeout(saveTimer.current);
    saveTimer.current = window.setTimeout(() => {
      saveSettings(settings).catch((error) =>
        console.error("Failed to save Eneye settings", error),
      );
    }, 300);
    return () => window.clearTimeout(saveTimer.current);
  }, [loaded, settings]);

  if (windowKind === "overlay") {
    return <OverlayWindow settings={settings} />;
  }

  if (!loaded) {
    return <div className="app-loading">正在打开 Eneye…</div>;
  }

  return (
    <>
      <SettingsWindow
        agentConnections={agentConnections}
        aiKeyConfigured={aiKeyConfigured}
        dictionaryStatus={dictionaryStatus}
        externalDictionaries={externalDictionaries}
        installedFonts={fonts}
        onAiKeyConfiguredChange={setAiKeyConfigured}
        onSettingsChange={setSettings}
        onExternalDictionariesChange={setExternalDictionaries}
        ocrAvailable={ocrAvailable}
        settings={settings}
      />
      {!isTauriRuntime() && (
        <aside className="browser-preview-label">界面预览</aside>
      )}
    </>
  );
}

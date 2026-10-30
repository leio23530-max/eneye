import { useState } from "react";
import { OverlayCard } from "./components/OverlayCard";
import { SettingsWindow } from "./components/SettingsWindow";
import {
  DEFAULT_SETTINGS,
  SAMPLE_LOOKUP,
  SAMPLE_TRANSLATION,
} from "./data/sample";
import { getWindowKind, isTauriRuntime } from "./lib/tauri";

export default function App() {
  const [settings, setSettings] = useState(DEFAULT_SETTINGS);
  const windowKind = getWindowKind();

  if (windowKind === "overlay") {
    return (
      <main className="overlay-stage">
        <OverlayCard
          lookup={SAMPLE_LOOKUP}
          settings={settings}
          translation={SAMPLE_TRANSLATION}
        />
      </main>
    );
  }

  return (
    <>
      <SettingsWindow onSettingsChange={setSettings} settings={settings} />
      {!isTauriRuntime() && (
        <aside className="browser-preview-label">界面预览</aside>
      )}
    </>
  );
}

import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { DEFAULT_SETTINGS, FONT_OPTIONS, SAMPLE_LOOKUP } from "../data/sample";
import type {
  AgentConnectionStatus,
  AppSettings,
  BootstrapData,
  HistoryRecord,
  ExternalDictionary,
  LookupResult,
  TranslationEvent,
  TranslationRequest,
  WordProbeReport,
} from "../types";
import { isTauriRuntime } from "./tauri";

const previewBootstrap: BootstrapData = {
  settings: DEFAULT_SETTINGS,
  fonts: FONT_OPTIONS.map((font) => font.family),
  dictionaryStatus: {
    available: true,
    name: "ECDICT",
    entryCount: 770_612,
    path: "浏览器预览数据",
  },
  externalDictionaries: [],
  agentConnections: [
    {
      id: "codex",
      name: "Codex",
      available: true,
      detail: "浏览器预览状态",
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
  ],
  aiKeyConfigured: false,
  ocrAvailable: true,
};

export async function loadBootstrap(): Promise<BootstrapData> {
  if (!isTauriRuntime()) return previewBootstrap;
  return invoke<BootstrapData>("get_bootstrap");
}

export async function detectAgentConnections(): Promise<
  AgentConnectionStatus[]
> {
  if (!isTauriRuntime()) return previewBootstrap.agentConnections;
  return invoke<AgentConnectionStatus[]>("detect_agent_connections");
}

export async function saveSettings(
  settings: AppSettings,
): Promise<AppSettings> {
  if (!isTauriRuntime()) return settings;
  return invoke<AppSettings>("save_settings", { settings });
}

export async function lookupWord(
  word: string,
  source = "uia",
): Promise<LookupResult> {
  if (!isTauriRuntime()) return SAMPLE_LOOKUP;
  return invoke<LookupResult>("lookup_word", { word, source });
}

export async function getHistory(
  query?: string,
  limit = 100,
): Promise<HistoryRecord[]> {
  if (!isTauriRuntime()) return [];
  return invoke<HistoryRecord[]>("get_history", {
    query: query?.trim() || null,
    limit,
  });
}

export async function getCaptureDiagnostics(): Promise<WordProbeReport | null> {
  if (!isTauriRuntime()) return null;
  return invoke<WordProbeReport>("get_capture_diagnostics");
}

export async function addExternalDictionary(
  path: string,
): Promise<ExternalDictionary> {
  if (!isTauriRuntime()) {
    const name =
      path
        .split(/[\\/]/)
        .pop()
        ?.replace(/\.[^.]+$/, "") ?? "词典";
    return {
      id: "preview",
      name,
      path,
      format: path.split(".").pop() ?? "unknown",
      priority: 0,
      available: true,
    };
  }
  return invoke<ExternalDictionary>("add_external_dictionary", { path });
}

export async function removeExternalDictionary(id: string): Promise<boolean> {
  if (!isTauriRuntime()) return true;
  return invoke<boolean>("remove_external_dictionary", { id });
}

export async function reorderExternalDictionaries(
  ids: string[],
): Promise<void> {
  if (!isTauriRuntime()) return;
  await invoke("reorder_external_dictionaries", { ids });
}

export async function deleteHistory(id: number): Promise<void> {
  if (!isTauriRuntime()) return;
  await invoke("delete_history", { id });
}

export async function clearHistory(): Promise<void> {
  if (!isTauriRuntime()) return;
  await invoke("clear_history");
}

export async function setAiKey(apiKey: string): Promise<boolean> {
  if (!isTauriRuntime()) return Boolean(apiKey.trim());
  return invoke<boolean>("set_ai_key", { apiKey });
}

export async function deleteAiKey(): Promise<void> {
  if (!isTauriRuntime()) return;
  await invoke("delete_ai_key");
}

export async function listAiModels(
  settings: AppSettings,
  apiKey?: string,
): Promise<string[]> {
  if (!isTauriRuntime()) return [];
  return invoke<string[]>("list_ai_models", {
    settings,
    apiKey: apiKey?.trim() || null,
  });
}

export async function translateSelection(
  request: TranslationRequest,
): Promise<void> {
  if (!isTauriRuntime()) return;
  await invoke("translate_selection", { request });
}

export async function cancelTranslation(requestId: string): Promise<void> {
  if (!isTauriRuntime()) return;
  await invoke("cancel_translation", { requestId });
}

export async function onTranslationEvent(
  handler: (event: TranslationEvent) => void,
): Promise<UnlistenFn> {
  if (!isTauriRuntime()) return () => undefined;
  return listen<TranslationEvent>("eneye://translation", (event) =>
    handler(event.payload),
  );
}

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
  Notebook,
  NotebookWord,
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

let previewNotebooks: Notebook[] = [
  {
    id: 1,
    name: "生词本",
    wordCount: 0,
    createdAt: "2026-01-01T00:00:00Z",
  },
  {
    id: 2,
    name: "好词本",
    wordCount: 0,
    createdAt: "2026-01-01T00:00:00Z",
  },
  {
    id: 3,
    name: "坏词本",
    wordCount: 0,
    createdAt: "2026-01-01T00:00:00Z",
  },
];
let previewNotebookWords: NotebookWord[] = [];
let previewNotebookId = 4;
let previewNotebookWordId = 1;

export function resetPreviewNotebooks() {
  previewNotebooks = [
    { id: 1, name: "生词本", wordCount: 0, createdAt: "2026-01-01T00:00:00Z" },
    { id: 2, name: "好词本", wordCount: 0, createdAt: "2026-01-01T00:00:00Z" },
    { id: 3, name: "坏词本", wordCount: 0, createdAt: "2026-01-01T00:00:00Z" },
  ];
  previewNotebookWords = [];
  previewNotebookId = 4;
  previewNotebookWordId = 1;
}

function previewNotebookCount(id: number) {
  return previewNotebookWords.filter((word) => word.notebookId === id).length;
}

export async function listNotebooks(): Promise<Notebook[]> {
  if (!isTauriRuntime()) {
    return previewNotebooks.map((notebook) => ({
      ...notebook,
      wordCount: previewNotebookCount(notebook.id),
    }));
  }
  return invoke<Notebook[]>("list_notebooks");
}

export async function createNotebook(name: string): Promise<Notebook> {
  if (!isTauriRuntime()) {
    const clean = name.trim();
    if (!clean) throw new Error("本子名称不能为空");
    if (
      previewNotebooks.some(
        (notebook) => notebook.name.toLowerCase() === clean.toLowerCase(),
      )
    ) {
      throw new Error("已有同名的本子");
    }
    const notebook: Notebook = {
      id: previewNotebookId++,
      name: clean,
      wordCount: 0,
      createdAt: new Date().toISOString(),
    };
    previewNotebooks.push(notebook);
    return notebook;
  }
  return invoke<Notebook>("create_notebook", { name });
}

export async function deleteNotebook(id: number): Promise<void> {
  if (!isTauriRuntime()) {
    previewNotebooks = previewNotebooks.filter(
      (notebook) => notebook.id !== id,
    );
    previewNotebookWords = previewNotebookWords.filter(
      (word) => word.notebookId !== id,
    );
    return;
  }
  await invoke("delete_notebook", { id });
}

export async function listNotebookWords(
  notebookId: number,
  query?: string,
): Promise<NotebookWord[]> {
  if (!isTauriRuntime()) {
    const needle = query?.trim().toLowerCase();
    return previewNotebookWords
      .filter((word) => word.notebookId === notebookId)
      .filter(
        (word) =>
          !needle ||
          word.word.toLowerCase().includes(needle) ||
          word.definition.toLowerCase().includes(needle),
      )
      .slice()
      .reverse();
  }
  return invoke<NotebookWord[]>("list_notebook_words", {
    notebookId,
    query: query?.trim() || null,
    limit: 200,
  });
}

export async function addWordToNotebook(
  notebookId: number,
  word: string,
  phonetic: string,
  definition: string,
): Promise<NotebookWord> {
  if (!isTauriRuntime()) {
    const clean = word.trim();
    const existing = previewNotebookWords.find(
      (item) =>
        item.notebookId === notebookId &&
        item.word.toLowerCase() === clean.toLowerCase(),
    );
    if (existing) {
      existing.phonetic = phonetic;
      existing.definition = definition;
      return { ...existing };
    }
    const entry: NotebookWord = {
      id: previewNotebookWordId++,
      notebookId,
      word: clean,
      phonetic,
      definition,
      createdAt: new Date().toISOString(),
    };
    previewNotebookWords.push(entry);
    return { ...entry };
  }
  return invoke<NotebookWord>("add_word_to_notebook", {
    notebookId,
    word,
    phonetic,
    definition,
  });
}

export async function removeNotebookWord(id: number): Promise<void> {
  if (!isTauriRuntime()) {
    previewNotebookWords = previewNotebookWords.filter(
      (word) => word.id !== id,
    );
    return;
  }
  await invoke("remove_notebook_word", { id });
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

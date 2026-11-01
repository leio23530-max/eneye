export type RecognitionSource =
  "uia" | "msaa" | "win32" | "clipboard" | "ocr" | "search";

export interface DictionaryEntry {
  dictionaryId: string;
  dictionaryName: string;
  priority: number;
  phonetic?: string;
  partOfSpeech?: string;
  definitions: string[];
  inflections?: string;
  examples: string[];
  html?: string;
}

export interface LookupResult {
  word: string;
  normalizedWord: string;
  source: RecognitionSource;
  entries: DictionaryEntry[];
}

export interface TranslationRequest {
  requestId: string;
  text: string;
  sourceLanguage: string;
  targetLanguage: string;
  source: RecognitionSource;
}

export type TranslationStatus = "idle" | "streaming" | "complete" | "error";

export interface TranslationState {
  requestId: string;
  text: string;
  status: TranslationStatus;
  output: string;
  error?: string;
}

export interface AppSettings {
  lookupDelayMs: number;
  wordCardLingerMs: number;
  overlayOffsetPx: number;
  aiTranslationEnabled: boolean;
  autoTranslate: boolean;
  launchAtStartup: boolean;
  headwordFont: string;
  bodyFont: string;
  sourceLanguage: string;
  targetLanguage: string;
  aiProtocol: "auto" | "openai" | "anthropic";
  aiConnectionName: string;
  aiBaseUrl: string;
  aiModel: string;
  aiRoles: AiRoles;
  unsupportedReasoningModels: string[];
}

export type AiEffort = "low" | "medium" | "high";

export interface AiRole {
  connectionId: string;
  model: string;
  effort: AiEffort;
}

export interface AiRoles {
  chat: AiRole;
  translation: AiRole;
  image: AiRole;
}

export interface FontOption {
  label: string;
  family: string;
}

export interface DictionaryStatus {
  available: boolean;
  name: string;
  entryCount: number;
  path: string;
  error?: string;
}

export interface BootstrapData {
  settings: AppSettings;
  fonts: string[];
  dictionaryStatus: DictionaryStatus;
  externalDictionaries: ExternalDictionary[];
  agentConnections: AgentConnectionStatus[];
  aiKeyConfigured: boolean;
  ocrAvailable: boolean;
}

export interface AgentConnectionStatus {
  id: string;
  name: string;
  available: boolean;
  detail: string;
}

export interface ExternalDictionary {
  id: string;
  name: string;
  path: string;
  format: string;
  priority: number;
  available: boolean;
  error?: string;
}

export interface Notebook {
  id: number;
  name: string;
  wordCount: number;
  createdAt: string;
}

export interface NotebookWord {
  id: number;
  notebookId: number;
  word: string;
  phonetic: string;
  definition: string;
  createdAt: string;
}

export interface HistoryRecord {
  id: number;
  kind: "word" | "sentence";
  text: string;
  translation: string;
  source: RecognitionSource;
  dictionaries: string[];
  createdAt: string;
}

export type TranslationEvent =
  | { type: "started"; requestId: string }
  | { type: "delta"; requestId: string; text: string }
  | { type: "completed"; requestId: string; text: string }
  | { type: "failed"; requestId: string; error: string }
  | { type: "cancelled"; requestId: string };

export interface SelectionPayload {
  requestId: string;
  text: string;
  source: RecognitionSource;
  sourceLanguage: string;
  targetLanguage: string;
}

export interface ProbeStage {
  provider: string;
  available: boolean;
  durationMs: number;
  detail: string;
}

export interface WordProbeReport {
  word?: string;
  source?: string;
  totalMs: number;
  stages: ProbeStage[];
}

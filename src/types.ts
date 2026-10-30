export type RecognitionSource = "uia" | "clipboard" | "ocr";

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
  autoTranslate: boolean;
  launchAtStartup: boolean;
  headwordFont: string;
  bodyFont: string;
  sourceLanguage: string;
  targetLanguage: string;
  aiProtocol: "auto" | "openai" | "anthropic";
  aiBaseUrl: string;
  aiModel: string;
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
  aiKeyConfigured: boolean;
  ocrAvailable: boolean;
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

export interface LookupLoadingPayload {
  source: RecognitionSource;
}

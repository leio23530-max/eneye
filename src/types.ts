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
  aiBaseUrl: string;
  aiModel: string;
}

export interface FontOption {
  label: string;
  family: string;
}

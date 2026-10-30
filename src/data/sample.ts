import type {
  AppSettings,
  FontOption,
  LookupResult,
  TranslationState,
} from "../types";

export const DEFAULT_SETTINGS: AppSettings = {
  lookupDelayMs: 240,
  autoTranslate: true,
  launchAtStartup: false,
  headwordFont: "Georgia",
  bodyFont: "Segoe UI",
  sourceLanguage: "auto",
  targetLanguage: "简体中文",
  aiProtocol: "auto",
  aiBaseUrl: "https://api.deepseek.com",
  aiModel: "",
};

export const FONT_OPTIONS: FontOption[] = [
  { label: "系统默认", family: "Segoe UI" },
  { label: "Georgia", family: "Georgia" },
  { label: "Cambria", family: "Cambria" },
  { label: "微软雅黑", family: "Microsoft YaHei UI" },
  { label: "宋体", family: "SimSun" },
  { label: "楷体", family: "KaiTi" },
  { label: "Consolas", family: "Consolas" },
  { label: "Times New Roman", family: "Times New Roman" },
];

export const SAMPLE_LOOKUP: LookupResult = {
  word: "serendipity",
  normalizedWord: "serendipity",
  source: "uia",
  entries: [
    {
      dictionaryId: "ecdict",
      dictionaryName: "ECDICT",
      priority: 100,
      phonetic: "/ˌserənˈdɪpəti/",
      partOfSpeech: "n.",
      definitions: ["意外发现美好事物的运气；机缘巧合"],
      inflections: "复数：serendipities",
      examples: [
        "Finding this quiet bookshop was pure serendipity.",
        "发现这家安静的书店纯属机缘巧合。",
      ],
    },
    {
      dictionaryId: "wordnet",
      dictionaryName: "English WordNet",
      priority: 200,
      partOfSpeech: "noun",
      definitions: ["good luck in making unexpected and fortunate discoveries"],
      examples: [],
    },
  ],
};

export const SAMPLE_TRANSLATION: TranslationState = {
  requestId: "sample",
  text: "A quiet interface should disappear into the act of reading.",
  status: "complete",
  output: "安静的界面应当消融于阅读行为本身。",
};

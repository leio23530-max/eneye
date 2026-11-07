import { useEffect, useRef, useState, type CSSProperties } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { Icon } from "./Icon";
import { IconButton } from "./IconButton";
import type {
  AppSettings,
  DictionaryEntry,
  LookupResult,
  Notebook,
  TranslationState,
} from "../types";
import { addWordToNotebook, listNotebooks } from "../lib/api";
import { isTauriRuntime } from "../lib/tauri";
import { DictionarySenseList } from "./DictionarySenseList";
import { PhoneticLines } from "./PhoneticLines";

interface OverlayCardProps {
  lookup: LookupResult;
  translation?: TranslationState;
  settings: AppSettings;
  onClose?: () => void;
  onPinChange?: (pinned: boolean) => void;
  onRetryTranslation?: () => void;
  onCancelTranslation?: () => void;
  onExplain?: (word: string) => void;
}

function useNearBottom(dependency: unknown) {
  const ref = useRef<HTMLDivElement>(null);
  const previousDependency = useRef(dependency);
  const [nearBottom, setNearBottom] = useState(true);
  const [showJump, setShowJump] = useState(false);

  useEffect(() => {
    if (Object.is(previousDependency.current, dependency)) return;
    previousDependency.current = dependency;
    const node = ref.current;
    if (!node || !nearBottom) return;
    if (typeof node.scrollTo === "function") {
      node.scrollTo({ top: node.scrollHeight, behavior: "smooth" });
    } else {
      node.scrollTop = node.scrollHeight;
    }
  }, [dependency, nearBottom]);

  function onScroll() {
    const node = ref.current;
    if (!node) return;
    const atBottom =
      node.scrollHeight - node.scrollTop - node.clientHeight < 36;
    setNearBottom(atBottom);
    setShowJump(!atBottom);
  }

  return {
    ref,
    showJump,
    onScroll,
    jump: () => {
      const node = ref.current;
      if (!node) return;
      if (typeof node.scrollTo === "function") {
        node.scrollTo({ top: node.scrollHeight, behavior: "smooth" });
      } else {
        node.scrollTop = node.scrollHeight;
      }
    },
  };
}

export function OverlayCard({
  lookup,
  translation,
  settings,
  onClose,
  onPinChange,
  onRetryTranslation,
  onCancelTranslation,
  onExplain,
}: OverlayCardProps) {
  const [pinned, setPinned] = useState(false);
  const [copied, setCopied] = useState(false);
  const [expanded, setExpanded] = useState<string | null>(null);
  const [bookmarkOpen, setBookmarkOpen] = useState(false);
  const [bookmarkBusy, setBookmarkBusy] = useState(false);
  const [bookmarkError, setBookmarkError] = useState("");
  const [savedTo, setSavedTo] = useState<string | null>(null);
  const [notebooks, setNotebooks] = useState<Notebook[]>([]);
  const {
    ref: scrollRef,
    showJump,
    onScroll,
    jump,
  } = useNearBottom(translation?.output);
  const main = lookup.entries[0];
  const others = lookup.entries.slice(1);
  const otherGroups: Array<{
    dictionaryId: string;
    dictionaryName: string;
    entries: DictionaryEntry[];
  }> = [];
  for (const entry of others) {
    const existing = otherGroups.find(
      (group) => group.dictionaryId === entry.dictionaryId,
    );
    if (existing) {
      existing.entries.push(entry);
    } else {
      otherGroups.push({
        dictionaryId: entry.dictionaryId,
        dictionaryName: entry.dictionaryName,
        entries: [entry],
      });
    }
  }

  useEffect(() => {
    function onKeyDown(event: KeyboardEvent) {
      if (event.key === "Escape") onClose?.();
    }
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [onClose]);

  function togglePin() {
    setPinned((current) => {
      onPinChange?.(!current);
      return !current;
    });
  }

  async function copyMainDefinition() {
    const content = [
      lookup.word,
      main?.phonetic,
      ...(main?.definitions ?? []),
      translation?.output,
    ]
      .filter(Boolean)
      .join("\n");
    await navigator.clipboard?.writeText(content);
    setCopied(true);
    window.setTimeout(() => setCopied(false), 1200);
  }

  function pronounceWord() {
    if (!("speechSynthesis" in window)) return;
    window.speechSynthesis.cancel();
    const utterance = new SpeechSynthesisUtterance(lookup.word);
    utterance.lang = "en-US";
    utterance.rate = 0.9;
    window.speechSynthesis.speak(utterance);
  }

  async function toggleBookmarkPicker() {
    if (bookmarkOpen) {
      setBookmarkOpen(false);
      return;
    }
    setBookmarkError("");
    setBookmarkOpen(true);
    try {
      setNotebooks(await listNotebooks());
    } catch (error) {
      setBookmarkError(String(error));
    }
  }

  async function saveToNotebook(notebook: Notebook) {
    if (bookmarkBusy) return;
    setBookmarkBusy(true);
    setBookmarkError("");
    try {
      const definition =
        main?.definitions.join("；") ?? translation?.output ?? "";
      await addWordToNotebook(
        notebook.id,
        lookup.word,
        main?.phonetic ?? "",
        definition,
      );
      setSavedTo(notebook.name);
      setBookmarkOpen(false);
    } catch (error) {
      setBookmarkError(String(error));
    } finally {
      setBookmarkBusy(false);
    }
  }

  function startDragging(event: React.MouseEvent<HTMLElement>) {
    if (!isTauriRuntime()) return;
    if ((event.target as HTMLElement).closest("button,a,input,textarea")) {
      return;
    }
    void getCurrentWindow().startDragging();
  }

  const cardStyle = {
    "--headword-font": `"${settings.headwordFont}", Georgia, serif`,
    "--body-font": `"${settings.bodyFont}", "Segoe UI", "Microsoft YaHei UI", sans-serif`,
  } as CSSProperties;

  return (
    <article
      aria-label={`${lookup.word} 的词典释义`}
      className="dictionary-card"
      data-pinned={pinned}
      style={cardStyle}
    >
      <header className="card-header" onMouseDown={startDragging}>
        <div className="headword-row">
          <div>
            <h1 className="headword">{lookup.word}</h1>
            <div className="pronunciation-row">
              <PhoneticLines value={main?.phonetic} />
              <button
                aria-label={`朗读 ${lookup.word}`}
                className="text-action pronunciation-action"
                onClick={pronounceWord}
                type="button"
              >
                <Icon name="volume" size={16} />
                朗读
              </button>
            </div>
          </div>
          <div className="card-actions">
            <IconButton
              active={Boolean(savedTo) || bookmarkOpen}
              icon="bookmark"
              label={savedTo ? `已收藏到 ${savedTo}` : "收藏到本子"}
              onClick={() => void toggleBookmarkPicker()}
            />
            <IconButton
              active={pinned}
              icon="pin"
              label={pinned ? "取消固定" : "固定卡片"}
              onClick={togglePin}
            />
            <IconButton
              icon={copied ? "check" : "copy"}
              label={copied ? "已复制" : "复制释义"}
              onClick={copyMainDefinition}
            />
            <IconButton icon="close" label="关闭卡片" onClick={onClose} />
          </div>
        </div>
      </header>

      {bookmarkOpen && (
        <div aria-label="收藏到本子" className="bookmark-popover" role="dialog">
          <div className="bookmark-popover-head">
            <strong>收藏到</strong>
            <button
              aria-label="关闭收藏面板"
              className="bookmark-close"
              onClick={() => setBookmarkOpen(false)}
              type="button"
            >
              <Icon name="close" size={14} />
            </button>
          </div>
          {notebooks.map((notebook) => (
            <button
              className="bookmark-option"
              disabled={bookmarkBusy}
              key={notebook.id}
              onClick={() => void saveToNotebook(notebook)}
              type="button"
            >
              <Icon name="book" size={15} />
              <span>{notebook.name}</span>
              <small>{notebook.wordCount}</small>
            </button>
          ))}
          {notebooks.length === 0 && (
            <p className="bookmark-empty">还没有本子，请到“生词本”里新建。</p>
          )}
          {bookmarkError && <p className="bookmark-error">{bookmarkError}</p>}
        </div>
      )}

      {savedTo && (
        <p className="bookmark-toast" role="status">
          已收藏到「{savedTo}」
        </p>
      )}

      <div
        aria-label="词典内容"
        className="card-scroll-area"
        onScroll={onScroll}
        ref={scrollRef}
        tabIndex={0}
      >
        {!main && (
          <section className="empty-definition">
            <div className="dictionary-label">
              <span>词典未收录</span>
              <span className="source-indicator">
                {lookup.source === "ocr" ? "屏幕识别" : "系统取词"}
              </span>
            </div>
            <p className="empty-definition-title">{lookup.word}</p>
            <p className="empty-definition-text">
              内置词典和已导入词典里都没有找到这个词。它可能是专业术语、新词或拼写变体。
            </p>
            {onExplain && (
              <button
                className="text-action empty-definition-action"
                onClick={() => onExplain(lookup.word)}
                type="button"
              >
                用 AI 解释
              </button>
            )}
          </section>
        )}

        {main && (
          <section className="primary-definition">
            <div className="dictionary-label">
              <span>{main.dictionaryName}</span>
              <span className="source-indicator">
                {lookup.source === "ocr" ? "屏幕识别" : "系统取词"}
              </span>
            </div>
            {main.html ? (
              <DictionarySenseList
                definitions={main.definitions}
                dictionaryId={main.dictionaryId}
                html={main.html}
              />
            ) : (
              <>
                <div className="definition-line">
                  {main.partOfSpeech && (
                    <span className="part-of-speech">{main.partOfSpeech}</span>
                  )}
                  <div className="definitions">
                    {main.definitions.map((definition, index) => (
                      <p key={definition + "-" + index}>{definition}</p>
                    ))}
                  </div>
                </div>
                {main.inflections && (
                  <p className="inflections">
                    <span>变形</span>
                    {main.inflections}
                  </p>
                )}
                {main.examples.length > 0 && (
                  <div className="example-block">
                    <p className="eyebrow">例句</p>
                    {main.examples.map((example, index) => (
                      <p
                        className={
                          index % 2 === 0 ? "example-en" : "example-zh"
                        }
                        key={example + "-" + index}
                      >
                        {example}
                      </p>
                    ))}
                  </div>
                )}
              </>
            )}
          </section>
        )}

        {others.length > 0 && (
          <section className="other-dictionaries">
            <p className="eyebrow">其他词典</p>
            {otherGroups.map((group) => {
              const isOpen = expanded === group.dictionaryId;
              const definitions = group.entries.flatMap(
                (entry) => entry.definitions,
              );
              const html = group.entries
                .map((entry) => entry.html)
                .filter((value): value is string => Boolean(value))
                .join("\n");
              const partsOfSpeech = Array.from(
                new Set(
                  group.entries
                    .map((entry) => entry.partOfSpeech)
                    .filter((value): value is string => Boolean(value)),
                ),
              );
              return (
                <div className="dictionary-accordion" key={group.dictionaryId}>
                  <button
                    aria-expanded={isOpen}
                    className="accordion-trigger"
                    onClick={() =>
                      setExpanded(isOpen ? null : group.dictionaryId)
                    }
                    type="button"
                  >
                    <span>{group.dictionaryName}</span>
                    <Icon
                      className={isOpen ? "chevron is-open" : "chevron"}
                      name="chevron"
                      size={16}
                    />
                  </button>
                  <div className="accordion-content" data-open={isOpen}>
                    {html ? (
                      <DictionarySenseList
                        definitions={definitions}
                        dictionaryId={group.dictionaryId}
                        html={html}
                      />
                    ) : (
                      <div>
                        {partsOfSpeech.length > 0 && (
                          <span className="part-of-speech">
                            {partsOfSpeech.join(" / ")}
                          </span>
                        )}
                        {definitions.map((definition, index) => (
                          <p key={definition + "-" + index}>{definition}</p>
                        ))}
                      </div>
                    )}
                  </div>
                </div>
              );
            })}
          </section>
        )}

        {translation && (
          <section
            aria-live="polite"
            aria-label="AI 翻译"
            className="translation-section"
          >
            <div className="section-heading">
              <div>
                <p className="eyebrow">
                  {translation.kind === "explain" ? "AI 解释" : "AI 翻译"}
                </p>
                <p className="translation-source">{translation.text}</p>
              </div>
              {translation.status === "streaming" && (
                <button
                  className="text-action"
                  onClick={onCancelTranslation}
                  type="button"
                >
                  <Icon name="pause" size={15} />
                  停止
                </button>
              )}
            </div>
            <p className="translation-output">
              {translation.output}
              {translation.status === "streaming" && (
                <span className="streaming-caret" aria-hidden="true" />
              )}
            </p>
            {translation.status === "error" && (
              <div className="inline-error" role="alert">
                <Icon name="alert" size={17} />
                <span>{translation.error ?? "翻译失败，请稍后重试。"}</span>
                <button
                  className="text-action"
                  onClick={onRetryTranslation}
                  type="button"
                >
                  重试
                </button>
              </div>
            )}
            {translation.status === "cancelled" && (
              <p className="translation-cancelled">
                已停止，保留已经生成的内容。
                <button
                  className="text-action"
                  onClick={onRetryTranslation}
                  type="button"
                >
                  重试
                </button>
              </p>
            )}
          </section>
        )}
      </div>

      {showJump && (
        <button className="jump-to-latest" onClick={jump} type="button">
          <Icon name="arrowDown" size={15} />
          回到最新内容
        </button>
      )}
    </article>
  );
}

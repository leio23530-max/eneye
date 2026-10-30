import { useEffect, useRef, useState, type CSSProperties } from "react";
import DOMPurify from "dompurify";
import { Icon } from "./Icon";
import { IconButton } from "./IconButton";
import type { AppSettings, LookupResult, TranslationState } from "../types";

interface OverlayCardProps {
  lookup: LookupResult;
  translation?: TranslationState;
  settings: AppSettings;
  onClose?: () => void;
  onPinChange?: (pinned: boolean) => void;
  onRetryTranslation?: () => void;
  onCancelTranslation?: () => void;
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
}: OverlayCardProps) {
  const [pinned, setPinned] = useState(false);
  const [copied, setCopied] = useState(false);
  const [expanded, setExpanded] = useState<string | null>(null);
  const {
    ref: scrollRef,
    showJump,
    onScroll,
    jump,
  } = useNearBottom(translation?.output);
  const main = lookup.entries[0];
  const others = lookup.entries.slice(1);

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
      <header className="card-header">
        <div className="headword-row">
          <div>
            <h1 className="headword">{lookup.word}</h1>
            <div className="pronunciation-row">
              {main?.phonetic && <span>{main.phonetic}</span>}
              <button
                aria-label={`朗读 ${lookup.word}`}
                className="text-action pronunciation-action"
                type="button"
              >
                <Icon name="volume" size={16} />
                朗读
              </button>
            </div>
          </div>
          <div className="card-actions">
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

      <div
        aria-label="词典内容"
        className="card-scroll-area"
        onScroll={onScroll}
        ref={scrollRef}
        tabIndex={0}
      >
        {main && (
          <section className="primary-definition">
            <div className="dictionary-label">
              <span>{main.dictionaryName}</span>
              <span className="source-indicator">
                {lookup.source === "ocr" ? "屏幕识别" : "系统取词"}
              </span>
            </div>
            <div className="definition-line">
              {main.partOfSpeech && (
                <span className="part-of-speech">{main.partOfSpeech}</span>
              )}
              <div className="definitions">
                {main.definitions.map((definition, index) => (
                  <p key={`${definition}-${index}`}>{definition}</p>
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
                    className={index % 2 === 0 ? "example-en" : "example-zh"}
                    key={`${example}-${index}`}
                  >
                    {example}
                  </p>
                ))}
              </div>
            )}
            {main.html && (
              <div
                className="rich-dictionary-content"
                dangerouslySetInnerHTML={{
                  __html: DOMPurify.sanitize(main.html, {
                    FORBID_TAGS: [
                      "script",
                      "form",
                      "iframe",
                      "object",
                      "embed",
                    ],
                    FORBID_ATTR: ["style"],
                  }),
                }}
              />
            )}
          </section>
        )}

        {others.length > 0 && (
          <section className="other-dictionaries">
            <p className="eyebrow">其他词典</p>
            {others.map((entry) => {
              const isOpen = expanded === entry.dictionaryId;
              return (
                <div className="dictionary-accordion" key={entry.dictionaryId}>
                  <button
                    aria-expanded={isOpen}
                    className="accordion-trigger"
                    onClick={() =>
                      setExpanded(isOpen ? null : entry.dictionaryId)
                    }
                    type="button"
                  >
                    <span>{entry.dictionaryName}</span>
                    <Icon
                      className={isOpen ? "chevron is-open" : "chevron"}
                      name="chevron"
                      size={16}
                    />
                  </button>
                  <div className="accordion-content" data-open={isOpen}>
                    <div>
                      {entry.partOfSpeech && (
                        <span className="part-of-speech">
                          {entry.partOfSpeech}
                        </span>
                      )}
                      {entry.definitions.map((definition) => (
                        <p key={definition}>{definition}</p>
                      ))}
                    </div>
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
                <p className="eyebrow">AI 翻译</p>
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

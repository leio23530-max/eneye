import { useMemo, useState } from "react";
import { parseDictionarySenses } from "../lib/dictionary-senses";
import { sanitizeDictionaryHtml } from "../lib/dictionary-html";
import { Icon } from "./Icon";

interface DictionarySenseListProps {
  html: string;
  dictionaryId: string;
  definitions?: string[];
}

export function DictionarySenseList({
  html,
  dictionaryId,
  definitions = [],
}: DictionarySenseListProps) {
  const senses = useMemo(() => parseDictionarySenses(html), [html]);
  const [openId, setOpenId] = useState<string | null>(null);

  if (senses.length === 0) {
    return (
      <div className="sense-fallback">
        {definitions.length > 0 && (
          <div className="definitions">
            {definitions.map((definition, index) => (
              <p key={definition + "-" + index}>{definition}</p>
            ))}
          </div>
        )}
        {html && (
          <details className="rich-dictionary-content">
            <summary>完整释义</summary>
            <div
              dangerouslySetInnerHTML={{
                __html: sanitizeDictionaryHtml(html, dictionaryId),
              }}
            />
          </details>
        )}
      </div>
    );
  }

  return (
    <div className="sense-list">
      {senses.map((sense) => {
        const isOpen = openId === sense.id;
        return (
          <section
            className={"sense-item" + (isOpen ? " is-open" : "")}
            key={sense.id}
          >
            <button
              aria-expanded={isOpen}
              className="sense-summary"
              onClick={() => setOpenId(isOpen ? null : sense.id)}
              type="button"
            >
              <span className="sense-number">{sense.number || "•"}</span>
              <span className="sense-copy">
                {sense.section && (
                  <span className="sense-section">
                    {sense.section}
                    {sense.sectionZh ? " · " + sense.sectionZh : ""}
                  </span>
                )}
                <strong>{sense.title}</strong>
                {sense.chinese && <small>{sense.chinese}</small>}
                {sense.meta && <em>{sense.meta}</em>}
              </span>
              <Icon
                className={isOpen ? "chevron is-open" : "chevron"}
                name="chevron"
                size={16}
              />
            </button>
            {isOpen && (
              <div
                className="sense-body"
                dangerouslySetInnerHTML={{
                  __html: sanitizeDictionaryHtml(sense.html, dictionaryId),
                }}
              />
            )}
          </section>
        );
      })}
    </div>
  );
}

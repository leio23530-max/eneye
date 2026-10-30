import { useMemo, useState } from "react";
import { parseDictionarySenses } from "../lib/dictionary-senses";
import { sanitizeDictionaryHtml } from "../lib/dictionary-html";
import { Icon } from "./Icon";

const POS_LABELS: Record<string, string> = {
  noun: "名词",
  verb: "动词",
  adjective: "形容词",
  adverb: "副词",
  "linking verb": "系动词",
  "modal verb": "情态动词",
  "auxiliary verb": "助动词",
  "phrasal verb": "短语动词",
  preposition: "介词",
  pronoun: "代词",
  conjunction: "连词",
  determiner: "限定词",
  exclamation: "感叹词",
  number: "数词",
};

function formatPos(pos: string): string {
  const normalized = pos.trim().toLowerCase();
  const chinese = POS_LABELS[normalized];
  return chinese ? pos + " · " + chinese : pos;
}

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
                {(sense.pos || sense.section) && (
                  <span className="sense-meta-line">
                    {sense.pos && (
                      <span className="sense-pos">{formatPos(sense.pos)}</span>
                    )}
                    {sense.section && (
                      <span className="sense-section">
                        {sense.section}
                        {sense.sectionZh ? " · " + sense.sectionZh : ""}
                      </span>
                    )}
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

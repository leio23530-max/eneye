import { useState, type FormEvent } from "react";
import { searchDictionary } from "../lib/api";
import type { LookupResult } from "../types";
import { DictionarySenseList } from "./DictionarySenseList";
import { Icon } from "./Icon";
import { PhoneticLines } from "./PhoneticLines";

export function DictionarySearchPanel() {
  const [query, setQuery] = useState("");
  const [result, setResult] = useState<LookupResult | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState("");

  async function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const clean = query.trim();
    if (!clean || loading) return;
    setLoading(true);
    setError("");
    try {
      setResult(await searchDictionary(clean));
    } catch (searchError) {
      setResult(null);
      setError(String(searchError));
    } finally {
      setLoading(false);
    }
  }

  return (
    <section className="settings-section dictionary-search">
      <div className="section-title">
        <div>
          <h2>查词</h2>
          <p>英文或中文都可以；中文会使用已安装的反查词典。</p>
        </div>
        <Icon name="search" />
      </div>
      <form className="dictionary-search-form" onSubmit={submit}>
        <label className="search-field">
          <Icon name="search" size={17} />
          <span className="sr-only">词典搜索</span>
          <input
            aria-label="词典搜索"
            onChange={(event) => setQuery(event.target.value)}
            placeholder="输入英文或中文，例如 serendipity / 美丽"
            type="search"
            value={query}
          />
        </label>
        <button
          className="primary-button"
          disabled={loading || !query.trim()}
          type="submit"
        >
          {loading ? "查询中…" : "查询"}
        </button>
      </form>

      {error && (
        <p className="inline-error" role="alert">
          <Icon name="alert" size={16} />
          {error}
        </p>
      )}

      {result && (
        <div
          aria-live="polite"
          className="dictionary-search-results"
          data-empty={result.entries.length === 0}
        >
          <p className="search-summary">
            “{result.word}” · {result.entries.length} 条结果
          </p>
          {result.entries.length === 0 ? (
            <div className="history-empty">
              <Icon name="book" size={24} />
              没有找到这个词，试试原形或其他写法。
            </div>
          ) : (
            result.entries.map((entry, index) => (
              <article
                className="search-entry"
                key={entry.dictionaryId + "-" + index}
              >
                <div className="search-entry-head">
                  <strong>{entry.dictionaryName}</strong>
                  <PhoneticLines value={entry.phonetic} />
                </div>
                {entry.html ? (
                  <DictionarySenseList
                    definitions={entry.definitions}
                    dictionaryId={entry.dictionaryId}
                    html={entry.html}
                  />
                ) : (
                  entry.definitions.length > 0 && (
                    <div className="search-entry-definitions">
                      {entry.definitions.map((definition, definitionIndex) => (
                        <p key={definition + "-" + definitionIndex}>
                          {definition}
                        </p>
                      ))}
                    </div>
                  )
                )}
              </article>
            ))
          )}
        </div>
      )}
    </section>
  );
}

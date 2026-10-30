const LATIN_WORD = /^[A-Za-z]+(?:['’\-][A-Za-z]+)*$/;
const CJK_CHARACTER = /[\u3400-\u4dbf\u4e00-\u9fff\uf900-\ufaff]/u;
const URL_LIKE = /^(?:https?:\/\/|www\.)\S+$/i;

export function cleanHoveredWord(raw: string): string | null {
  if (/[0-9_]/.test(raw)) return null;
  const candidate = raw
    .trim()
    .replace(/^[^A-Za-z]+|[^A-Za-z]+$/g, "")
    .replace(/’/g, "'");

  return LATIN_WORD.test(candidate) ? candidate : null;
}

export function normalizeHeadword(raw: string): string {
  return raw.trim().replace(/’/g, "'").toLocaleLowerCase("en-US");
}

export function isUsableSelection(raw: string): boolean {
  const text = raw.trim().replace(/\s+/g, " ");
  if (!text || text.length > 2000 || URL_LIKE.test(text)) return false;

  const latinWords = text.match(/[A-Za-z]+(?:['’\-][A-Za-z]+)*/g) ?? [];
  if (latinWords.length >= 2) return true;

  const cjkCharacters = Array.from(text).filter((character) =>
    CJK_CHARACTER.test(character),
  );
  return cjkCharacters.length >= 4;
}

export function selectionWordCount(raw: string): number {
  return (raw.match(/[A-Za-z]+(?:['’\-][A-Za-z]+)*/g) ?? []).length;
}

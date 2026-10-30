import { describe, expect, it } from "vitest";
import { cleanHoveredWord, isUsableSelection, normalizeHeadword } from "./text";

describe("text helpers", () => {
  it("cleans punctuation around a hovered word", () => {
    expect(cleanHoveredWord("“Serendipity,”")).toBe("Serendipity");
    expect(cleanHoveredWord("don’t")).toBe("don't");
  });

  it("rejects non-English hover candidates", () => {
    expect(cleanHoveredWord("你好")).toBeNull();
    expect(cleanHoveredWord("hello123")).toBeNull();
  });

  it("normalizes headwords without changing the display form", () => {
    expect(normalizeHeadword("Don’t")).toBe("don't");
  });

  it("auto-translates a sentence but not one word or a URL", () => {
    expect(isUsableSelection("A quiet reading room")).toBe(true);
    expect(isUsableSelection("hello")).toBe(false);
    expect(isUsableSelection("https://example.com/read")).toBe(false);
    expect(isUsableSelection("这是一段中文文字。")).toBe(true);
  });
});

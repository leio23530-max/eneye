import { describe, expect, it } from "vitest";
import { sanitizeDictionaryHtml } from "./dictionary-html";

describe("sanitizeDictionaryHtml", () => {
  it("removes scripts and rewrites MDD resources", () => {
    const result = sanitizeDictionaryHtml(
      '<script>alert(1)</script><img src="file:///images/icon.png">',
      "dictionary-1",
    );
    expect(result).not.toContain("<script");
    expect(result).toContain(
      "http://eneye-dict.localhost/dictionary-1/images/icon.png",
    );
  });

  it("removes navigation links", () => {
    const result = sanitizeDictionaryHtml(
      '<a href="https://evil.example">link</a>',
      "dictionary-1",
    );
    expect(result).not.toContain("href=");
  });
});

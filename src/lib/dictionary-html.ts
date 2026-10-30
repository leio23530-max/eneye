import DOMPurify from "dompurify";

const LOCAL_RESOURCE = /^(?:file:\/\/\/?|[\\/])?(.+)$/i;

export function sanitizeDictionaryHtml(
  html: string,
  dictionaryId: string,
): string {
  const document = new DOMParser().parseFromString(html, "text/html");
  document.querySelectorAll<HTMLElement>("[src]").forEach((element) => {
    const source = element.getAttribute("src");
    if (source) {
      element.setAttribute("src", localResourceUrl(source, dictionaryId));
    }
  });
  document.querySelectorAll<HTMLAnchorElement>("a[href]").forEach((anchor) => {
    anchor.removeAttribute("href");
    anchor.setAttribute("rel", "noreferrer noopener");
  });
  return DOMPurify.sanitize(document.body.innerHTML, {
    FORBID_TAGS: [
      "script",
      "form",
      "iframe",
      "object",
      "embed",
      "link",
      "base",
    ],
    FORBID_ATTR: ["style", "srcset"],
  });
}

function localResourceUrl(source: string, dictionaryId: string): string {
  if (/^(?:data:|blob:|http:\/\/eneye-dict\.localhost\/)/i.test(source)) {
    return source;
  }
  const match = source.match(LOCAL_RESOURCE);
  const relativePath = (match?.[1] ?? source)
    .replace(/\\/g, "/")
    .replace(/^\/+/, "");
  const encodedPath = relativePath
    .split("/")
    .filter(Boolean)
    .map(encodeURIComponent)
    .join("/");
  return `http://eneye-dict.localhost/${encodeURIComponent(dictionaryId)}/${encodedPath}`;
}

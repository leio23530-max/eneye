export interface DictionarySense {
  id: string;
  section: string;
  sectionZh: string;
  number: string;
  title: string;
  chinese: string;
  pos: string;
  meta: string;
  html: string;
}

const MAX_SENSES = 120;

function compact(value: string | null | undefined): string {
  return (value ?? "").replace(/\s+/g, " ").trim();
}

function headingText(element: Element): string {
  const clone = element.cloneNode(true) as HTMLElement;
  clone.querySelectorAll("shcut").forEach((child) => child.remove());
  return compact(clone.textContent);
}

export function parseDictionarySenses(html: string): DictionarySense[] {
  if (!html.trim()) return [];
  const document = new DOMParser().parseFromString(html, "text/html");
  const senses: DictionarySense[] = [];
  const seen = new Set<string>();
  let pos = "";
  let section = "";
  let sectionZh = "";

  const walker = document.createTreeWalker(
    document.body,
    NodeFilter.SHOW_ELEMENT,
  );
  while (walker.nextNode()) {
    const element = walker.currentNode as Element;
    if (element.matches("div.entry")) {
      const posElement =
        element.querySelector(":scope > .top-container .pos") ??
        element.querySelector(".pos");
      pos = compact(posElement?.textContent);
      section = "";
      sectionZh = "";
      continue;
    }
    if (element.matches("h2.shcut")) {
      section = headingText(element);
      sectionZh = compact(element.querySelector("shcut chn")?.textContent);
      continue;
    }
    if (!element.matches("li.sense")) continue;
    if (senses.length >= MAX_SENSES) break;

    const idiom = compact(
      element.closest(".idm-g")?.querySelector(".idm")?.textContent,
    );
    const number = compact(
      element.querySelector(":scope > .iteration")?.textContent,
    );
    const definition = compact(
      element.querySelector(":scope > .sensetop .def")?.textContent ??
        element.querySelector(".def")?.textContent,
    );
    const chinese = compact(
      element.querySelector(":scope > .sensetop deft chn")?.textContent ??
        element.querySelector("deft chn")?.textContent,
    );
    const meta = compact(
      Array.from(
        element.querySelectorAll(
          ":scope > .grammar, :scope > .cf, :scope > .sensetop > .grammar, :scope > .sensetop > .cf, :scope > .sensetop > .labels",
        ),
      )
        .map((node) => node.textContent)
        .slice(0, 2)
        .join(" · "),
    );

    const title = idiom || definition || section || "释义";
    const dedupeKey = [pos, section, idiom, title, chinese, meta].join("|");
    if (seen.has(dedupeKey)) continue;
    seen.add(dedupeKey);
    senses.push({
      id: (element.id || "sense") + "-" + senses.length,
      section: idiom ? "习语" : section,
      sectionZh: idiom ? "" : sectionZh,
      number: number === "❑" ? "" : number,
      title,
      chinese,
      pos,
      meta: idiom ? definition : meta,
      html: element.outerHTML,
    });
  }
  return senses;
}

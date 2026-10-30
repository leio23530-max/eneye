import { describe, expect, it } from "vitest";
import { parseDictionarySenses } from "./dictionary-senses";

const HTML = `<html><body>
<div class="entry"><div class="top-container"><span class="pos">verb</span></div>
<h2 class="shcut">in position<shcut><chn>位置</chn></shcut></h2>
<ol class="senses_multiple">
  <li class="sense" id="place_sng_1">
    <span class="iteration">1</span>
    <span class="sensetop"><span class="grammar">[transitive]</span><span class="def">to put something somewhere</span><deft><chn>放置</chn></deft></span>
    <ul class="examples"><li><span class="x">Place it here.</span></li></ul>
  </li>
  <li class="sense" id="place_sng_2">
    <span class="iteration">2</span>
    <span class="sensetop"><span class="def">a particular position</span><deft><chn>位置</chn></deft></span>
  </li>
</ol>
<span class="idm-g"><div class="webtop"><span class="idm">in place</span></div>
  <ol><li class="sense"><span class="sensetop"><span class="def">ready</span><deft><chn>就绪</chn></deft></span></li></ol>
</span>
</div>
</body></html>`;

describe("parseDictionarySenses", () => {
  it("extracts sections, definitions, translations, and idioms", () => {
    const senses = parseDictionarySenses(HTML);
    expect(senses).toHaveLength(3);
    expect(senses[0].pos).toBe("verb");
    expect(senses[0].section).toBe("in position");
    expect(senses[0].sectionZh).toBe("位置");
    expect(senses[0].title).toBe("to put something somewhere");
    expect(senses[0].chinese).toBe("放置");
    expect(senses[0].meta).toContain("[transitive]");
    expect(senses[1].number).toBe("2");
    expect(senses[2].section).toBe("习语");
    expect(senses[2].title).toBe("in place");
    expect(senses[2].meta).toBe("ready");
  });
});

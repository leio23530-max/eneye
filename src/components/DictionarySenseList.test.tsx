import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it } from "vitest";
import { DictionarySenseList } from "./DictionarySenseList";

const HTML = `<html><body>
<div class="entry"><div class="top-container"><span class="pos">verb</span></div>
<h2 class="shcut">in position<shcut><chn>位置</chn></shcut></h2>
<li class="sense"><span class="iteration">1</span><span class="sensetop"><span class="def">to put something somewhere</span><deft><chn>放置</chn></deft></span><ul class="examples"><li><span class="x">Place it here.</span></li></ul></li>
<li class="sense"><span class="iteration">2</span><span class="sensetop"><span class="def">a particular position</span><deft><chn>位置</chn></deft></span></li>
</div>
</body></html>`;

describe("DictionarySenseList", () => {
  it("keeps senses collapsed and expands only the chosen one", async () => {
    const user = userEvent.setup();
    render(<DictionarySenseList dictionaryId="test" html={HTML} />);

    const first = screen.getByRole("button", {
      name: /to put something somewhere/,
    });
    const second = screen.getByRole("button", {
      name: /a particular position/,
    });
    expect(first).toHaveTextContent("verb · 动词");
    expect(first).toHaveAttribute("aria-expanded", "false");
    expect(screen.queryByText("Place it here.")).not.toBeInTheDocument();

    await user.click(first);
    expect(first).toHaveAttribute("aria-expanded", "true");
    expect(screen.getByText("Place it here.")).toBeVisible();

    await user.click(second);
    expect(second).toHaveAttribute("aria-expanded", "true");
    expect(first).toHaveAttribute("aria-expanded", "false");
    expect(screen.queryByText("Place it here.")).not.toBeInTheDocument();
  });
});

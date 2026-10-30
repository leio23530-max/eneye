import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { PhoneticLines } from "./PhoneticLines";

describe("PhoneticLines", () => {
  it("renders each phonetic variant on its own line", () => {
    render(<PhoneticLines value={"/ɔːˈtɒnəmi/\n/ɔːˈtɑːnəmi/"} />);
    expect(screen.getByText("/ɔːˈtɒnəmi/")).toBeVisible();
    expect(screen.getByText("/ɔːˈtɑːnəmi/")).toBeVisible();
  });

  it("renders nothing without a phonetic", () => {
    const { container } = render(<PhoneticLines />);
    expect(container.firstChild).toBeNull();
  });
});

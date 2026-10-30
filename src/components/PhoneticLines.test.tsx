import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { PhoneticLines } from "./PhoneticLines";

describe("PhoneticLines", () => {
  it("renders each phonetic variant on its own line", () => {
    render(<PhoneticLines value={"/ɔːˈtɒnəmi/\n/ɔːˈtɑːnəmi/"} />);
    expect(screen.getByText("/ɔːˈtɒnəmi/")).toBeVisible();
    expect(screen.getByText("/ɔːˈtɑːnəmi/")).toBeVisible();
  });

  it("keeps accent labels intact", () => {
    render(<PhoneticLines value={"英音 /ˈhelpə(r)/\n美音 /ˈhelpər/"} />);
    expect(screen.getByText("英音 /ˈhelpə(r)/")).toBeVisible();
    expect(screen.getByText("美音 /ˈhelpər/")).toBeVisible();
  });

  it("wraps bare ECDICT phonetics in slashes", () => {
    render(<PhoneticLines value="wud" />);
    expect(screen.getByText("/wud/")).toBeVisible();
  });

  it("renders nothing without a phonetic", () => {
    const { container } = render(<PhoneticLines />);
    expect(container.firstChild).toBeNull();
  });
});

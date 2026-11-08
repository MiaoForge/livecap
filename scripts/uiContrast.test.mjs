import { describe, expect, it } from "vitest";
import { readFileSync } from "node:fs";
const css = readFileSync(new URL("../src/Client.css", import.meta.url), "utf8");

function property(selector, name) {
  const values = css
    .split(`\n${selector} {`)
    .slice(1)
    .flatMap((block) =>
      block
        .split("}")[0]
        .split("\n")
        .map((line) => line.trim())
        .filter((line) => line.startsWith(`${name}:`))
        .map((line) =>
          line
            .slice(name.length + 1)
            .trim()
            .replace(/;$/, ""),
        ),
    );
  return values[values.length - 1] ?? "";
}

function luminance(hex) {
  expect(hex).toMatch(/^#[\da-f]{6}$/i);
  const rgb = [1, 3, 5].map((start) => {
    const value = parseInt(hex.slice(start, start + 2), 16) / 255;
    return value <= 0.04045 ? value / 12.92 : ((value + 0.055) / 1.055) ** 2.4;
  });
  return rgb[0] * 0.2126 + rgb[1] * 0.7152 + rgb[2] * 0.0722;
}

function contrast(foreground, background) {
  const a = luminance(foreground);
  const b = luminance(background);
  return (Math.max(a, b) + 0.05) / (Math.min(a, b) + 0.05);
}

describe("sidebar text contrast", () => {
  for (const selector of [".nav-label", ".workspace small", "nav a"]) {
    it(`${selector} meets the 4.5:1 normal text threshold`, () => {
      expect(
        contrast(
          property(selector, "color"),
          property(".sidebar", "background"),
        ),
      ).toBeGreaterThanOrEqual(4.5);
    });
  }
  it("keeps navigation text readable on hover", () => {
    expect(
      contrast(
        property("nav a", "color"),
        property("nav a:hover", "background"),
      ),
    ).toBeGreaterThanOrEqual(4.5);
  });
});

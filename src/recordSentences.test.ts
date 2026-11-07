import { expect, it } from "vitest";
import { timedSentences } from "./recordSentences";

it("keeps estimated sentences inside the recorded paragraph and preserves the text", () => {
  const text = "Short one. This is a substantially longer second sentence.";
  const result = timedSentences(text, 6000, 21000);
  expect(result).toHaveLength(2);
  expect(result.map((sentence) => sentence.text).join("")).toBe(text);
  expect(result[0].startMs).toBe(6000);
  expect(result[0].endMs).toBe(result[1].startMs);
  expect(result[1].endMs).toBe(21000);
  expect(result[1].startMs).toBeLessThan(13500);
  expect(timedSentences(text)[0].startMs).toBeUndefined();
});

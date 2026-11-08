import { describe, expect, it } from "vitest";
import { createCaptionState } from "./captions";
import type { CaptionSegment } from "./captions";
import { selectSessionTranscript } from "./sessionPresentation";

function source(id: string, text: string, language = "en"): CaptionSegment {
  return { id, text, language, revision: 1, lane: "original" };
}
function target(id: string, text: string, sourceId?: string): CaptionSegment {
  return {
    id,
    text,
    revision: 1,
    lane: "translation",
    ...(sourceId
      ? {
          externalTranslation: {
            sessionId: "test",
            sourceSegmentId: sourceId,
            revision: 1,
          },
        }
      : {}),
  };
}
function state() {
  return { ...createCaptionState(), sessionId: "test" };
}

describe("session transcript presentation", () => {
  it("separates original speech and translations while retaining provisional text", () => {
    const captions = state();
    captions.confirmed = [
      { ...source("one", "Original"), speaker: "2", startMs: 1000 },
      target("translation", "译文", "one"),
    ];
    captions.drafts = [
      source("next", "Speaking"),
      target("draft", "翻译中", "next"),
    ];
    const result = selectSessionTranscript(captions, "zh");
    expect(result.originals.map((line) => line.segment.text)).toEqual([
      "Original",
      "Speaking",
    ]);
    expect(result.translations.map((line) => line.segment.text)).toEqual([
      "译文",
      "翻译中",
    ]);
    expect(result.translations[1]).toMatchObject({
      draft: true,
      sourceId: "next",
    });
  });
  it("inherits speaker and timing from an explicitly linked original", () => {
    const captions = state();
    captions.confirmed = [
      { ...source("one", "Original"), speaker: "2", startMs: 1000 },
      target("translation", "译文", "one"),
    ];
    expect(
      selectSessionTranscript(captions, "zh").translations[0].segment,
    ).toMatchObject({ speaker: "2", startMs: 1000 });
  });
  it("orders late translations by their original speech rather than completion time", () => {
    const captions = state();
    captions.confirmed = [
      source("first", "First"),
      source("second", "Second"),
      target("second-target", "第二", "second"),
      target("first-target", "第一", "first"),
    ];
    expect(
      selectSessionTranscript(captions, "zh").translations.map(
        (line) => line.sourceId,
      ),
    ).toEqual(["first", "second"]);
  });
  it("shows speech already in the target language without waiting for a translation", () => {
    const captions = state();
    captions.confirmed = [source("native", "原本就是中文", "zh")];
    const result = selectSessionTranscript(captions, "zh");
    expect(result.translations[0]).toMatchObject({
      sourceId: "native",
      segment: { text: "原本就是中文" },
    });
    captions.confirmed.push(target("duplicate", "原本就是中文", "native"));
    expect(selectSessionTranscript(captions, "zh").translations).toHaveLength(
      1,
    );
  });
  it("leaves unaligned provider lanes unpaired and ignores links from other sessions", () => {
    const captions = state();
    const stale = target("stale", "旧会话引用", "original");
    stale.externalTranslation!.sessionId = "other-session";
    captions.confirmed = [
      source("original", "Original"),
      target("unlinked", "独立译文"),
      stale,
    ];
    const result = selectSessionTranscript(captions, "zh");
    expect(result.translations.every((line) => line.sourceId == null)).toBe(
      true,
    );
    expect(result.translations.map((line) => line.segment.id)).toEqual([
      "unlinked",
      "stale",
    ]);
  });
});

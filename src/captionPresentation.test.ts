import { describe, expect, it } from "vitest";
import { selectLiveCaptions } from "./captionPresentation";
import {
  applyCaptionTokens,
  createCaptionState,
  type TranscriptToken,
} from "./captions";

function source(
  id: string,
  text: string,
  revision: number,
  isFinal = false,
): TranscriptToken {
  return {
    text,
    is_final: isFinal,
    source_segment_id: id,
    revision,
    translation_status: "original",
  };
}

function target(
  id: string,
  text: string,
  revision: number,
  isFinal = false,
): TranscriptToken {
  return {
    text,
    is_final: isFinal,
    source_segment_id: id,
    revision,
    translation_status: "translation",
  };
}

describe("live caption selection", () => {
  it("chooses a newer confirmed translation over the older draft still waiting for its final", () => {
    const state = applyCaptionTokens(createCaptionState(), [
      source("old", "First", 1),
      target("old", "第一", 1),
      source("old", "First.", 2, true),
      source("new", "Second.", 1, true),
      target("new", "第二。", 1, true),
    ]);

    expect(state.drafts.map(({ text }) => text)).toContain("第一");
    const current = selectLiveCaptions(state);
    expect(current.target).toMatchObject({
      segment: { text: "第二。" },
      draft: false,
    });
    expect(current.matchedSource).toMatchObject({
      segment: { text: "Second." },
      draft: false,
    });
    expect(current.sourceInProgress).toBeUndefined();
  });

  it("keeps pairing when a source draft and its translation settle in separate frames", () => {
    let state = applyCaptionTokens(createCaptionState(), [
      source("turn", "Clang", 1),
      target("turn", "Clang", 1),
    ]);
    const initial = selectLiveCaptions(state);
    expect(initial.target?.draft).toBe(true);
    expect(initial.matchedSource).toMatchObject({
      segment: { text: "Clang" },
      draft: true,
    });

    state = applyCaptionTokens(state, [
      source("turn", "Clang crashed.", 2, true),
    ]);
    const pending = selectLiveCaptions(state);
    expect(pending.target?.segment.id).toBe(initial.target?.segment.id);
    expect(pending.matchedSource).toMatchObject({
      segment: { text: "Clang crashed." },
      draft: false,
    });

    state = applyCaptionTokens(state, [
      target("turn", "Clang 崩溃了。", 2, true),
    ]);
    const finished = selectLiveCaptions(state);
    expect(finished.target).toMatchObject({
      segment: { text: "Clang 崩溃了。" },
      draft: false,
    });
    expect(finished.target?.segment.providerItemId).toBeUndefined();
    expect(finished.matchedSource?.segment.id).toBe(
      pending.matchedSource?.segment.id,
    );
  });

  it("keeps the matched source beside Chinese and exposes newer English separately", () => {
    const state = applyCaptionTokens(createCaptionState(), [
      source("old", "First.", 1, true),
      target("old", "第一。", 1, true),
      source("new", "Next", 1),
    ]);
    const current = selectLiveCaptions(state);
    expect(current.matchedSource?.segment.text).toBe("First.");
    expect(current.latestSource).toMatchObject({
      segment: { text: "Next" },
      draft: true,
    });
    expect(current.sourceInProgress).toBe(current.latestSource);

    const pending = selectLiveCaptions(
      applyCaptionTokens(state, [source("new", "Next.", 2, true)]),
    );
    expect(pending.target?.segment.text).toBe("第一。");
    expect(pending.sourceInProgress).toMatchObject({
      segment: { text: "Next." },
      draft: false,
    });
  });

  it("shows independent Soniox lanes without attaching an unlinked translation to English", () => {
    const state = applyCaptionTokens(createCaptionState(), [
      { text: "Earlier", is_final: true, speaker: "1" },
      { text: "稍早", is_final: true, translation_status: "translation" },
      { text: "Later", is_final: false, speaker: "2" },
      { text: "最新译文", is_final: false, translation_status: "translation" },
    ]);
    const current = selectLiveCaptions(state);
    expect(current.target).toMatchObject({
      segment: { text: "最新译文" },
      draft: true,
    });
    expect(current.latestSource).toMatchObject({
      segment: { text: "Later" },
      draft: true,
    });
    expect(current.matchedSource).toBeUndefined();
    expect(current.sourceInProgress).toBeUndefined();
  });

  it("supports an empty window or a source still waiting for its first translation", () => {
    const state = createCaptionState();
    expect(selectLiveCaptions(state)).toEqual({});
    const current = selectLiveCaptions(
      applyCaptionTokens(state, [source("turn", "Hello", 1)]),
    );
    expect(current.target).toBeUndefined();
    expect(current.latestSource?.segment.text).toBe("Hello");
  });
});

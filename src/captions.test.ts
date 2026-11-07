import { describe, expect, it } from "vitest";
import syntheticSonioxFrames from "../test-fixtures/soniox-stream.json";
import {
  applyCaptionFrame,
  applyCaptionTokens,
  applyTextTranslation,
  clearCaptionDrafts,
  createCaptionState,
  createDemoCaptions,
  createTextTranslationRequest,
  editCaptionSegment,
  exportCaptions,
  freezeCaptionSegment,
  HISTORY_LIMIT,
  renameSpeaker,
  speakerLabel,
  type TextTranslationAdapter,
  type TranscriptFrame,
  type TranscriptToken,
} from "./captions";

const token = (
  text: string,
  details: Partial<TranscriptToken> = {},
): TranscriptToken => ({
  text,
  is_final: true,
  ...details,
});

describe("synthetic Soniox stream", () => {
  it("feeds mixed native frames into displayed lanes and exports the finished text", () => {
    let state = createCaptionState();
    const frames = syntheticSonioxFrames.map((response): TranscriptFrame => ({
      session_id: state.sessionId,
      tokens: (response.tokens as TranscriptToken[]).map((token) =>
        token.translation_status === "translation"
          ? { ...token, translation_source: "soniox" }
          : token,
      ),
      finished: response.finished ?? false,
    }));
    state = applyCaptionFrame(state, frames[0]);
    expect(state.drafts.map(({ text }) => text)).toEqual(["The claim"]);
    state = applyCaptionFrame(state, frames[1]);
    expect(state.drafts.map(({ text }) => text)).toEqual([" crashes", "Clang"]);
    const firstSegmentId = state.confirmed[0].id;
    state = applyCaptionFrame(state, frames[2]);
    expect(state.confirmed).toMatchObject([
      {
        id: firstSegmentId,
        lane: "original",
        text: "Clang crashes.",
        speaker: "1",
        startMs: 0,
        endMs: 650,
      },
      { lane: "original", text: "In Sema?", speaker: "2" },
      {
        lane: "translation",
        text: "Clang 崩溃了。",
        speaker: undefined,
        startMs: undefined,
      },
    ]);
    expect(state.drafts.map(({ text }) => text)).toEqual(["在"]);

    state = applyCaptionFrame(state, frames[3]);
    expect(state.drafts).toEqual([]);
    expect(state.open).toEqual({});
    expect(exportCaptions(state)).toBe(
      [
        "原文",
        "说话人 1  Clang crashes.",
        "说话人 2  In Sema?",
        "",
        "译文",
        "说话人未识别  Clang 崩溃了。",
        "说话人 2  在 Sema 中吗？",
      ].join("\n"),
    );
    expect(
      applyCaptionFrame(state, {
        tokens: null,
        finished: true,
      } as unknown as TranscriptFrame),
    ).toBe(state);
  });
});

describe("normalized OpenAI stream", () => {
  const source = (
    id: string,
    text: string,
    isFinal = true,
    revision = 1,
  ): TranscriptToken => ({
    text,
    is_final: isFinal,
    source_segment_id: id,
    revision,
    translation_status: "original",
  });
  const translation = (
    id: string,
    text: string,
    isFinal = true,
    revision = 1,
  ): TranscriptToken => ({
    text,
    is_final: isFinal,
    source_segment_id: id,
    revision,
    translation_status: "translation",
    translation_source: "openai",
    language: "zh",
  });
  function frame(
    state: ReturnType<typeof createCaptionState>,
    ...tokens: TranscriptToken[]
  ) {
    return { session_id: state.sessionId, tokens, finished: false };
  }

  it("replaces keyed drafts and keeps adjacent completed turns distinct and idempotent", () => {
    let state = createCaptionState();
    state = applyCaptionFrame(
      state,
      frame(state, source("item_1", "Cl", false)),
    );
    const draftId = state.drafts[0].id;
    state = applyCaptionFrame(
      state,
      frame(state, source("item_1", "Clang", false, 2)),
    );
    expect(state.drafts.map(({ id, text }) => ({ id, text }))).toEqual([
      { id: draftId, text: "Clang" },
    ]);
    state = applyCaptionFrame(
      state,
      frame(
        state,
        source("item_1", "Clang.", false, 3),
        source("item_1", "Clang.", true, 4),
      ),
    );
    expect(state.drafts).toEqual([]);
    state = applyCaptionFrame(
      state,
      frame(state, source("item_1", "Clang."), source("item_2", "Sema.")),
    );
    expect(state.confirmed.map(({ text }) => text)).toEqual([
      "Clang.",
      "Sema.",
    ]);
    expect(state.confirmed.every(({ speaker }) => speaker === undefined)).toBe(
      true,
    );
  });

  it("preserves other pending items when an interleaved draft is updated or finalized", () => {
    let state = createCaptionState();
    state = applyCaptionFrame(
      state,
      frame(state, source("item_1", "Cl", false)),
    );
    state = applyCaptionFrame(
      state,
      frame(state, source("item_2", "Se", false)),
    );
    state = applyCaptionFrame(
      state,
      frame(state, source("item_1", "Clang", false, 2)),
    );
    expect(state.drafts.map(({ text }) => text)).toEqual(["Clang", "Se"]);
    state = applyCaptionFrame(
      state,
      frame(state, source("item_1", "Clang.", true, 3)),
    );
    state = applyCaptionFrame(
      state,
      frame(
        state,
        source("item_1", "late", false),
        translation("item_1", "编译器。"),
      ),
    );
    expect(state.drafts.map(({ text }) => text)).toEqual(["Se"]);
    state = applyCaptionFrame(state, frame(state, source("item_2", "Sema.")));
    expect(state.drafts).toEqual([]);
    expect(
      state.confirmed
        .filter(({ lane }) => lane === "original")
        .map(({ text }) => text),
    ).toEqual(["Clang.", "Sema."]);

    state = applyCaptionFrame(
      state,
      frame(state, source("noise", "uh", false)),
    );
    state = applyCaptionFrame(state, frame(state, source("noise", "")));
    state = applyCaptionFrame(
      state,
      frame(state, source("noise", "uh", false, 2)),
    );
    expect(state.drafts).toEqual([]);
    expect(exportCaptions(state)).not.toContain("uh");
  });

  it("shows only the newest provisional translation while the source and final translation are in flight", () => {
    let state = createCaptionState();
    const step = (...tokens: TranscriptToken[]) => {
      state = applyCaptionFrame(state, frame(state, ...tokens));
    };
    const zh = () => state.drafts.find(({ lane }) => lane === "translation");
    step(
      source("item_1", "Clang", false, 1),
      source("item_1", "Clang crashes", false, 3),
    );
    step(translation("item_1", "Clang", false, 1));
    const draftId = zh()?.id;
    expect(zh()).toMatchObject({ text: "Clang", revision: 1 });
    expect(exportCaptions(state)).toBe("");
    step(
      source("item_1", "stale", false, 2),
      translation("item_1", "Clang 崩溃", false, 3),
    );
    step(
      translation("item_1", "旧内容", false, 2),
      translation("item_1", "重复版本", false, 3),
    );
    step(translation("item_1", "未来版本", false, 5));
    expect(zh()).toMatchObject({
      id: draftId,
      text: "Clang 崩溃",
      revision: 3,
    });
    expect(state.drafts.find(({ lane }) => lane === "original")?.text).toBe(
      "Clang crashes",
    );

    step(source("item_1", "Clang crashes in Sema", false, 4));
    step(source("item_1", "Clang crashes in Sema.", true, 5));
    expect(state.drafts).toEqual([
      expect.objectContaining({ id: draftId, text: "Clang 崩溃" }),
    ]);
    step(translation("item_1", "Clang 在 Sema 中崩溃", false, 4));
    step(translation("item_1", "错误定稿", true, 4));
    expect(zh()).toMatchObject({ id: draftId, revision: 4 });
    expect(exportCaptions(state)).not.toContain("崩溃");
    step(translation("item_1", "Clang 在 Sema 中崩溃。", true, 5));
    step(translation("item_1", "定稿后迟到的草稿", false, 4));
    expect(state.drafts).toEqual([]);
    expect(exportCaptions(state)).toContain("Clang 在 Sema 中崩溃。");
    state = applyCaptionFrame(state, { ...frame(state), finished: true });
    expect(
      applyCaptionFrame(state, frame(state, source("item_2", "late", false))),
    ).toBe(state);
  });

  it("clears provisional translations on an empty final and on a manual source edit", () => {
    let state = createCaptionState();
    const step = (...tokens: TranscriptToken[]) => {
      state = applyCaptionFrame(state, frame(state, ...tokens));
    };
    step(
      source("failed", "Clang", false, 1),
      translation("failed", "草稿一", false, 1),
    );
    step(
      source("edited", "Sema", false, 1),
      translation("edited", "草稿二", false, 1),
    );
    step(
      source("failed", "Clang.", true, 3),
      source("edited", "Sema.", true, 3),
    );
    step(
      translation("failed", "", true, 3),
      translation("failed", "失败后迟到", false, 2),
    );
    expect(state.drafts.map(({ text }) => text)).toEqual(["草稿二"]);

    const edited = state.confirmed.find(
      ({ providerItemId }) => providerItemId === "edited",
    )!;
    state = editCaptionSegment(state, edited.id, { text: "Sema analysis." });
    step(translation("edited", "旧原文定稿", true, 3));
    step(translation("edited", "恰好匹配本地修订号", true, 4));
    expect(state.drafts).toEqual([]);
    expect(state.confirmed.map(({ text }) => text)).toEqual([
      "Clang.",
      "Sema analysis.",
    ]);
  });

  it("links out-of-order translations to their exact source and ignores old session frames", () => {
    const oldSession = createCaptionState();
    let state = createCaptionState();
    state = applyCaptionFrame(
      state,
      frame(state, source("item_1", "Clang."), source("item_2", "Sema.")),
    );
    const [first, second] = state.confirmed;
    expect(
      applyCaptionFrame(
        state,
        frame(oldSession, translation("item_1", "过期")),
      ),
    ).toBe(state);
    state = applyCaptionFrame(
      state,
      frame(state, translation("item_2", "语义分析。")),
    );
    state = applyCaptionFrame(
      state,
      frame(state, translation("item_1", "Clang 编译器。")),
    );
    expect(
      state.confirmed.filter(({ lane }) => lane === "translation"),
    ).toMatchObject([
      {
        text: "Clang 编译器。",
        externalTranslation: { sourceSegmentId: first.id },
      },
      {
        text: "语义分析。",
        externalTranslation: { sourceSegmentId: second.id },
      },
    ]);
    expect(exportCaptions(state)).toContain("说话人未识别  Clang 编译器。");
  });

  it("keeps source and translation edits when duplicate or outdated provider events arrive", () => {
    let state = createCaptionState();
    state = applyCaptionFrame(
      state,
      frame(state, source("item_1", "Clam"), source("item_2", "Sema.")),
    );
    state = editCaptionSegment(state, state.confirmed[0].id, {
      text: "Clang.",
    });
    state = applyCaptionFrame(
      state,
      frame(
        state,
        source("item_1", "Clam"),
        translation("item_1", "过期"),
        translation("item_2", "语义分析。"),
      ),
    );
    const target = state.confirmed.find(({ lane }) => lane === "translation")!;
    state = editCaptionSegment(state, target.id, { text: "Sema 阶段。" });
    state = applyCaptionFrame(
      state,
      frame(state, translation("item_2", "过期")),
    );
    expect(state.confirmed.map(({ text }) => text)).toEqual([
      "Clang.",
      "Sema.",
      "Sema 阶段。",
    ]);
  });
});

describe("external text translation seam", () => {
  const options = {
    sourceLanguage: "en",
    targetLanguage: "zh",
    glossary: ["Clang", "Sema"],
  };
  const fakeTranslator: TextTranslationAdapter = async ({
    sessionId,
    sourceSegmentId,
    revision,
  }) => ({
    sessionId,
    sourceSegmentId,
    revision,
    text: "请检查 Sema。",
  });

  it("uses confirmed original text, context, and explicit attribution", async () => {
    const context = Array.from({ length: 4 }, (_, index) =>
      token(String(index).repeat(220), {
        speaker: String((index % 2) + 1),
      }),
    );
    let state = applyCaptionTokens(createCaptionState(), [
      ...context,
      token("Check Sema.", { speaker: "1", language: "en" }),
      token("检查", { translation_status: "translation" }),
      token(" next", { is_final: false }),
    ]);
    const source = state.confirmed[4];
    const request = createTextTranslationRequest(state, source.id, options)!;
    expect(request).toMatchObject({
      text: "Check Sema.",
      sourceLanguage: "en",
      targetLanguage: "zh",
      speaker: "1",
      glossary: ["Clang", "Sema"],
      precedingContext: ["1", "2", "3"].map((value) => value.repeat(200)),
    });
    expect(
      createTextTranslationRequest(state, state.drafts[0].id, options),
    ).toBeUndefined();
    expect(
      createTextTranslationRequest(state, state.confirmed[5].id, options),
    ).toBeUndefined();

    const result = await fakeTranslator(request);
    state = applyTextTranslation(state, request, result);
    expect(state.confirmed[state.confirmed.length - 1]).toMatchObject({
      lane: "translation",
      speaker: "1",
      text: result.text,
      language: "zh",
      externalTranslation: {
        sessionId: state.sessionId,
        sourceSegmentId: source.id,
        revision: source.revision,
      },
    });
    expect(applyTextTranslation(state, request, result)).toBe(state);
  });

  it("rejects stale results across appends, source edits, and sessions with reused IDs", async () => {
    let state = applyCaptionTokens(createCaptionState(), [token("Check")]);
    const sourceId = state.confirmed[0].id;
    const draftRequest = createTextTranslationRequest(
      state,
      sourceId,
      options,
    )!;
    const draftResult = await fakeTranslator(draftRequest);
    state = applyCaptionTokens(state, [token(" Sema.")]);
    expect(applyTextTranslation(state, draftRequest, draftResult)).toBe(state);

    const fullRequest = createTextTranslationRequest(state, sourceId, options)!;
    const fullResult = await fakeTranslator(fullRequest);
    state = applyTextTranslation(state, fullRequest, fullResult);
    expect(state.confirmed).toHaveLength(2);
    state = editCaptionSegment(state, sourceId, { text: "Inspect Sema." });
    expect(state.confirmed).toHaveLength(1);
    expect(applyTextTranslation(state, fullRequest, fullResult)).toBe(state);

    const newSession = applyCaptionTokens(createCaptionState(), [
      token("Check"),
      token(" Sema."),
    ]);
    expect(newSession.confirmed[0]).toMatchObject({
      id: sourceId,
      revision: fullRequest.revision,
    });
    expect(applyTextTranslation(newSession, fullRequest, fullResult)).toBe(
      newSession,
    );
  });

  it("keeps manual translation corrections when duplicate or newer results arrive", async () => {
    let state = applyCaptionTokens(createCaptionState(), [
      token("Check Sema."),
    ]);
    const sourceId = state.confirmed[0].id;
    const request = createTextTranslationRequest(state, sourceId, options)!;
    const result = await fakeTranslator(request);
    state = applyTextTranslation(state, request, result);
    state = editCaptionSegment(state, state.confirmed[1].id, {
      text: "请查看 Sema。",
    });
    expect(applyTextTranslation(state, request, result)).toBe(state);

    state = editCaptionSegment(state, sourceId, { text: "Inspect Sema." });
    const newer = createTextTranslationRequest(state, sourceId, options)!;
    expect(
      applyTextTranslation(state, newer, await fakeTranslator(newer)),
    ).toBe(state);
    expect(exportCaptions(state)).toContain("请查看 Sema。");
  });
});

describe("caption state", () => {
  it("replaces each draft and appends final tokens once", () => {
    let state = applyCaptionTokens(createCaptionState(), [
      token("clang", { is_final: false }),
    ]);
    state = applyCaptionTokens(state, [
      token("Clang seg", { is_final: false }),
    ]);
    expect(state.drafts.map((segment) => segment.text)).toEqual(["Clang seg"]);
    expect(exportCaptions(state)).toBe("");

    state = applyCaptionTokens(state, [
      token("Clang"),
      token(" crashes", { is_final: false }),
    ]);
    expect(state.confirmed.map((segment) => segment.text)).toEqual(["Clang"]);
    expect(state.drafts.map((segment) => segment.text)).toEqual([" crashes"]);
    state = applyCaptionTokens(state, [token(" crashes")]);
    expect(state.confirmed.map((segment) => segment.text)).toEqual([
      "Clang crashes",
    ]);
    expect(state.drafts).toEqual([]);
  });

  it("keeps lagged translations independent of speakers and source timings", () => {
    let state = applyCaptionTokens(createCaptionState(), [
      token("First", {
        speaker: "1",
        start_ms: 100,
        end_ms: 400,
        translation_status: "original",
      }),
      token("Second", {
        speaker: "2",
        start_ms: 450,
        end_ms: 600,
        translation_status: "none",
      }),
      token("第一", { is_final: false, translation_status: "translation" }),
    ]);
    state = applyCaptionTokens(state, [
      token(" is speaking", { speaker: "2", start_ms: 620, end_ms: 1000 }),
      token("第一个说话者", { translation_status: "translation" }),
      token("第二个说话者", {
        speaker: "2",
        translation_status: "translation",
      }),
    ]);
    expect(state.confirmed).toMatchObject([
      {
        lane: "original",
        text: "First",
        speaker: "1",
        startMs: 100,
        endMs: 400,
      },
      {
        lane: "original",
        text: "Second is speaking",
        speaker: "2",
        startMs: 450,
        endMs: 1000,
      },
      {
        lane: "translation",
        text: "第一个说话者",
        speaker: undefined,
        startMs: undefined,
      },
      { lane: "translation", text: "第二个说话者", speaker: "2" },
    ]);
    expect(state.drafts).toEqual([]);
  });

  it("keeps lane fragments together across mixed responses and splits on boundaries", () => {
    const translated = {
      translation_status: "translation",
      speaker: "1",
    } as const;
    const state = applyCaptionTokens(createCaptionState(), [
      token("Hel", { speaker: "1", start_ms: 0, end_ms: 10 }),
      token("你", translated),
      token("lo", { speaker: "1", start_ms: 20, end_ms: 30 }),
      token("好", translated),
      token("Again", { speaker: "1", start_ms: 2000, end_ms: 2100 }),
      token("<end>"),
      token("After endpoint", { speaker: "1", start_ms: 2200, end_ms: 2300 }),
      token("<fin>"),
      token("After finalization", {
        speaker: "1",
        start_ms: 2400,
        end_ms: 2500,
      }),
      token("No label"),
    ]);
    expect(state.confirmed.map((segment) => segment.text)).toEqual([
      "Hello",
      "你好",
      "Again",
      "After endpoint",
      "After finalization",
      "No label",
    ]);
    expect(
      speakerLabel(state, state.confirmed[state.confirmed.length - 1]?.speaker),
    ).toBe("说话人未识别");
  });

  it("keeps corrections and speaker names stable as more tokens arrive", () => {
    let state = applyCaptionTokens(createCaptionState(), [
      token("seamer", { speaker: "1" }),
    ]);
    const { id } = state.confirmed[0];
    state = editCaptionSegment(state, id, { text: "Sema", speaker: "2" });
    state = renameSpeaker(state, "2", "  Chen  ");
    state = applyCaptionTokens(state, [token(" continues", { speaker: "1" })]);
    expect(state.confirmed[0]).toMatchObject({
      id,
      text: "Sema",
      speaker: "2",
    });
    expect(state.confirmed[1].id).not.toBe(id);
    expect(exportCaptions(state)).toContain("Chen  Sema");
    expect(speakerLabel(renameSpeaker(state, "2", ""), "2")).toBe("说话人 2");
    expect(
      editCaptionSegment(state, id, { speaker: null }).confirmed[0].speaker,
    ).toBeUndefined();
  });

  it("keeps arrivals separate while a correction is in progress", () => {
    const state = applyCaptionTokens(createCaptionState(), [token("Clam")]);
    const { id } = state.confirmed[0];
    const queued = applyCaptionTokens(state, [token(" parses")]);
    const incoming = applyCaptionTokens(freezeCaptionSegment(queued, id), [
      token(" templates"),
    ]);
    const corrected = editCaptionSegment(incoming, id, {
      text: "Clang",
      baseText: "Clam",
    });
    expect(corrected.confirmed.map((segment) => segment.text)).toEqual([
      "Clang parses",
      " templates",
    ]);
  });

  it("preserves independent drafts in a response that also closes a confirmed utterance", () => {
    const state = applyCaptionTokens(createCaptionState(), [
      token("First speaker", { speaker: "1" }),
      token("第一", { is_final: false, translation_status: "translation" }),
      token("<end>"),
      token("Next speaker", { is_final: false, speaker: "2" }),
    ]);
    expect(state.confirmed.map((segment) => segment.text)).toEqual([
      "First speaker",
    ]);
    expect(state.drafts.map((segment) => segment.text)).toEqual([
      "第一",
      "Next speaker",
    ]);
  });

  it("clears drafts and closes open segments at the end of a session", () => {
    const state = applyCaptionTokens(createCaptionState(), [
      token("one"),
      token(" two", { is_final: false }),
    ]);
    const stopped = clearCaptionDrafts(state);
    expect(stopped.drafts).toEqual([]);
    expect(
      applyCaptionTokens(stopped, [token("three")]).confirmed.map(
        (segment) => segment.text,
      ),
    ).toEqual(["one", "three"]);
  });

  it("caps confirmed history while keeping recent IDs and reporting losses", () => {
    const tokens = Array.from({ length: HISTORY_LIMIT + 3 }, (_, index) =>
      token(`line ${index}`, {
        speaker: String((index % 2) + 1),
      }),
    );
    let state = applyCaptionTokens(createCaptionState(), tokens);
    expect(state.confirmed).toHaveLength(HISTORY_LIMIT);
    expect(state.droppedSegments).toBe(3);
    expect(state.confirmed[0].text).toBe("line 3");
    const recent = state.confirmed[HISTORY_LIMIT - 1];
    state = applyCaptionTokens(state, [
      token(" continued", { speaker: recent.speaker }),
    ]);
    expect(state.confirmed[HISTORY_LIMIT - 1]).toMatchObject({
      id: recent.id,
      text: `${recent.text} continued`,
    });
    expect(exportCaptions(state)).toContain("较早的 3 段已移除");
  });

  it("provides two local demo speakers with both lanes and unexported drafts", () => {
    const demo = createDemoCaptions();
    expect(new Set(demo.confirmed.map((segment) => segment.speaker))).toEqual(
      new Set(["1", "2"]),
    );
    expect(demo.drafts).toHaveLength(2);
    const exported = exportCaptions(demo);
    expect(exported).toContain("LLVM");
    expect(exported).toContain("译文");
    expect(exported).not.toContain(demo.drafts[0].text);
  });
});

it("keeps complete archived records while limiting the live display", () => {
  let live = createCaptionState();
  let archive = createCaptionState(Number.MAX_SAFE_INTEGER);
  for (let i = 0; i < HISTORY_LIMIT + 20; i++) {
    const tokens: TranscriptToken[] = [
      {
        text: `Sentence ${i}.`,
        is_final: true,
        translation_status: "original",
        source_segment_id: `item-${i}`,
        revision: 1,
      },
    ];
    live = applyCaptionTokens(live, tokens);
    archive = applyCaptionTokens(archive, tokens);
  }
  expect(live.confirmed).toHaveLength(HISTORY_LIMIT);
  expect(archive.confirmed).toHaveLength(HISTORY_LIMIT + 20);
  expect(archive.confirmed[0].text).toBe("Sentence 0.");
  expect(archive.droppedSegments).toBe(0);
});

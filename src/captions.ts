export interface TranscriptToken {
  text: string;
  is_final: boolean;
  speaker?: string;
  translation_status?: "none" | "original" | "translation";
  start_ms?: number;
  end_ms?: number;
  language?: string;
  source_language?: string;
  translation_source?: "soniox" | "openai";
  source_segment_id?: string;
  revision?: number;
}

export interface TranscriptFrame {
  tokens: readonly TranscriptToken[];
  finished: boolean;
  session_id: string;
}

type CaptionLane = "original" | "translation";

interface TranslationKey {
  sessionId: string;
  sourceSegmentId: string;
  revision: number;
}

export interface TextTranslationRequest extends TranslationKey {
  text: string;
  sourceLanguage: string;
  targetLanguage: string;
  speaker?: string;
  glossary: readonly string[];
  precedingContext: readonly string[];
}

export interface TextTranslationResult extends TranslationKey {
  text: string;
}

// Future adapters invoke a native command. Credentials and network calls stay in Rust.
export type TextTranslationAdapter = (
  request: TextTranslationRequest,
) => Promise<TextTranslationResult>;

export interface CaptionSegment {
  id: string;
  revision: number;
  lane: CaptionLane;
  speaker?: string;
  text: string;
  startMs?: number;
  endMs?: number;
  language?: string;
  providerItemId?: string;
  providerRevision?: number;
  translationSettled?: boolean;
  externalTranslation?: TranslationKey & { edited?: boolean };
}

export interface CaptionState {
  historyLimit?: number;
  sessionId: string;
  confirmed: CaptionSegment[];
  drafts: CaptionSegment[];
  speakerNames: Record<string, string>;
  droppedSegments: number;
  nextId: number;
  open: Partial<Record<CaptionLane, string>>;
  emptyFinalItems: string[];
  finished: boolean;
}

export const HISTORY_LIMIT = 500;
const SEGMENT_LENGTH = 280;
const SEGMENT_GAP_MS = 1500;

export function createCaptionState(historyLimit = HISTORY_LIMIT): CaptionState {
  return {
    historyLimit,
    sessionId: crypto.randomUUID(),
    confirmed: [],
    drafts: [],
    speakerNames: {},
    droppedSegments: 0,
    nextId: 1,
    open: {},
    emptyFinalItems: [],
    finished: false,
  };
}

export function applyCaptionFrame(
  state: CaptionState,
  frame: TranscriptFrame,
): CaptionState {
  if (!Array.isArray(frame?.tokens)) return state;
  if (frame.session_id !== state.sessionId || state.finished) return state;
  const next = applyCaptionTokens(state, frame.tokens);
  return frame.finished
    ? { ...clearCaptionDrafts(next), finished: true }
    : next;
}

function retainCurrentTranslations(
  segments: CaptionSegment[],
): CaptionSegment[] {
  return segments.filter(({ externalTranslation }) => {
    if (!externalTranslation || externalTranslation.edited) return true;
    const source = segments.find(
      ({ id }) => id === externalTranslation.sourceSegmentId,
    );
    return !source || source.revision === externalTranslation.revision;
  });
}

export function applyCaptionTokens(
  state: CaptionState,
  tokens: readonly TranscriptToken[],
): CaptionState {
  const confirmed = state.confirmed.slice();
  // Soniox replaces its whole draft snapshot. Keyed OpenAI updates replace one item.
  const drafts = state.drafts.filter(
    ({ providerItemId }) => providerItemId !== undefined,
  );
  const open = { ...state.open };
  const openDrafts: Partial<Record<CaptionLane, string>> = {};
  let nextId = state.nextId;
  let emptyFinalItems = state.emptyFinalItems;

  for (const token of tokens) {
    const lane =
      token.translation_status === "translation" ? "translation" : "original";
    const itemId = token.source_segment_id;
    if (itemId) {
      const revision = token.revision;
      if (
        revision === undefined ||
        !Number.isSafeInteger(revision) ||
        revision < 1 ||
        emptyFinalItems.includes(itemId)
      )
        continue;
      const sourceIndex = confirmed.findIndex(
        (segment) =>
          segment.lane === "original" && segment.providerItemId === itemId,
      );
      const source = confirmed[sourceIndex];
      const draftIndex = drafts.findIndex(
        (draft) => draft.providerItemId === itemId && draft.lane === lane,
      );
      const draft = drafts[draftIndex];

      if (lane === "translation") {
        if (
          source &&
          (source.revision !== source.providerRevision ||
            source.translationSettled ||
            confirmed.some(
              ({ externalTranslation }) =>
                externalTranslation?.sourceSegmentId === source.id,
            ))
        )
          continue;
        if (token.is_final) {
          if (!source || revision !== source.providerRevision) continue;
          if (draftIndex >= 0) drafts.splice(draftIndex, 1);
          confirmed[sourceIndex] = { ...source, translationSettled: true };
          if (token.text.trim()) {
            confirmed.splice(sourceIndex + 1, 0, {
              id: `segment-${nextId++}`,
              revision: 1,
              lane,
              text: token.text,
              speaker: source.speaker,
              language: token.language,
              externalTranslation: {
                sessionId: state.sessionId,
                sourceSegmentId: source.id,
                revision,
              },
            });
          }
          continue;
        }
        const sourceDraft = drafts.find(
          (segment) =>
            segment.providerItemId === itemId && segment.lane === "original",
        );
        // A usable draft may have been requested before the latest source delta.
        const latestDraftRevision = source
          ? source.revision - 1
          : (sourceDraft?.revision ?? 0);
        if (
          revision > latestDraftRevision ||
          revision <= (draft?.revision ?? 0)
        )
          continue;
      } else {
        if (source || revision < (draft?.revision ?? 0)) continue;
        if (token.is_final) {
          if (draftIndex >= 0) drafts.splice(draftIndex, 1);
          if (token.text.trim()) {
            confirmed.push({
              id: `segment-${nextId++}`,
              revision,
              lane,
              text: token.text,
              providerItemId: itemId,
              providerRevision: revision,
              speaker: token.speaker,
              language: token.language,
              startMs: token.start_ms,
              endMs: token.end_ms,
            });
          } else {
            const targetIndex = drafts.findIndex(
              (segment) => segment.providerItemId === itemId,
            );
            if (targetIndex >= 0) drafts.splice(targetIndex, 1);
            emptyFinalItems = [...emptyFinalItems, itemId].slice(
              -HISTORY_LIMIT,
            );
          }
          continue;
        }
        if (revision === draft?.revision) continue;
      }
      if (token.text.trim()) {
        const nextDraft: CaptionSegment = {
          id: `draft-item-${lane}-${itemId}`,
          revision,
          lane,
          text: token.text,
          providerItemId: itemId,
          speaker: token.speaker,
          language: token.language,
        };
        if (draftIndex < 0) drafts.push(nextDraft);
        else drafts[draftIndex] = nextDraft;
      }
      continue;
    }

    // Soniox supplies no alignment IDs or timestamps for translated tokens.
    // Keep each lane in its own sequence and retain only explicit speaker labels.
    if (token.text === "<end>" || token.text === "<fin>") {
      if (token.is_final) {
        delete open.original;
        delete open.translation;
      }
      continue;
    }
    if (!token.text) continue;

    const segments = token.is_final ? confirmed : drafts;
    const openSegments = token.is_final ? open : openDrafts;
    const index = segments.findIndex(
      (segment) => segment.id === openSegments[lane],
    );
    const previous = segments[index];
    const gap =
      token.start_ms !== undefined && previous?.endMs !== undefined
        ? token.start_ms - previous.endMs
        : 0;

    if (
      previous &&
      previous.speaker === token.speaker &&
      previous.language === token.language &&
      gap <= SEGMENT_GAP_MS &&
      previous.text.length < SEGMENT_LENGTH
    ) {
      segments[index] = {
        ...previous,
        revision: previous.revision + 1,
        text: previous.text + token.text,
        endMs: token.end_ms ?? previous.endMs,
      };
      continue;
    }

    const id = token.is_final
      ? `segment-${nextId++}`
      : `draft-${drafts.length}`;
    segments.push({
      id,
      revision: 1,
      lane,
      text: token.text,
      speaker: token.speaker,
      language: token.language,
      startMs: token.start_ms,
      endMs: token.end_ms,
    });
    openSegments[lane] = id;
  }

  const current = retainCurrentTranslations(confirmed);
  const dropped = Math.max(
    0,
    current.length - (state.historyLimit ?? HISTORY_LIMIT),
  );
  return {
    ...state,
    confirmed: current.slice(dropped),
    drafts: drafts.filter(({ text }) => text.trim()).slice(-HISTORY_LIMIT),
    open,
    nextId,
    emptyFinalItems,
    droppedSegments: state.droppedSegments + dropped,
  };
}

export function clearCaptionDrafts(state: CaptionState): CaptionState {
  return { ...state, drafts: [], open: {} };
}

export function freezeCaptionSegment(
  state: CaptionState,
  id: string,
): CaptionState {
  const lane =
    state.open.original === id
      ? "original"
      : state.open.translation === id
        ? "translation"
        : undefined;
  if (!lane) return state;
  const open = { ...state.open };
  delete open[lane];
  return { ...state, open };
}

export function editCaptionSegment(
  state: CaptionState,
  id: string,
  edit: { text?: string; speaker?: string | null; baseText?: string },
): CaptionState {
  const original = state.confirmed.find((segment) => segment.id === id);
  if (!original) return state;

  // An update already queued when the edit opened may have extended the segment.
  const appended =
    edit.baseText !== undefined && original.text.startsWith(edit.baseText)
      ? original.text.slice(edit.baseText.length)
      : "";
  const corrected: CaptionSegment = {
    ...original,
    revision: original.revision + 1,
    text: edit.text === undefined ? original.text : edit.text + appended,
    speaker:
      edit.speaker === undefined
        ? original.speaker
        : edit.speaker?.trim() || undefined,
  };
  if (original.externalTranslation)
    corrected.externalTranslation = {
      ...original.externalTranslation,
      edited: true,
    };
  return {
    ...freezeCaptionSegment(state, id),
    confirmed: retainCurrentTranslations(
      state.confirmed.map((segment) =>
        segment.id === id ? corrected : segment,
      ),
    ),
    drafts: original.providerItemId
      ? state.drafts.filter(
          ({ providerItemId }) => providerItemId !== original.providerItemId,
        )
      : state.drafts,
  };
}

export function createTextTranslationRequest(
  state: CaptionState,
  id: string,
  options: {
    sourceLanguage: string;
    targetLanguage: string;
    glossary: readonly string[];
  },
): TextTranslationRequest | undefined {
  const index = state.confirmed.findIndex(
    (segment) => segment.id === id && segment.lane === "original",
  );
  const source = state.confirmed[index];
  if (!source?.text.trim()) return undefined;
  const precedingContext = state.confirmed
    .slice(0, index)
    .filter((segment) => segment.lane === "original")
    .slice(-3)
    .map(({ text }) => text.trim().slice(-200))
    .filter(Boolean);
  return {
    sessionId: state.sessionId,
    sourceSegmentId: id,
    revision: source.revision,
    text: source.text.trim(),
    speaker: source.speaker,
    sourceLanguage: source.language ?? options.sourceLanguage,
    targetLanguage: options.targetLanguage,
    glossary: options.glossary.slice(0, 100),
    precedingContext,
  };
}

export function applyTextTranslation(
  state: CaptionState,
  request: TextTranslationRequest,
  result: TextTranslationResult,
): CaptionState {
  const { sessionId, sourceSegmentId, revision } = result;
  const text = typeof result.text === "string" ? result.text.trim() : "";
  if (
    !text ||
    sessionId !== state.sessionId ||
    sessionId !== request.sessionId ||
    sourceSegmentId !== request.sourceSegmentId ||
    revision !== request.revision
  )
    return state;
  const source = state.confirmed.find(
    ({ id, lane }) => id === sourceSegmentId && lane === "original",
  );
  if (
    !source ||
    source.revision !== revision ||
    source.text.trim() !== request.text
  )
    return state;
  if (
    state.confirmed.some(
      ({ externalTranslation }) =>
        externalTranslation?.sourceSegmentId === sourceSegmentId,
    )
  ) {
    return state;
  }

  const translation: CaptionSegment = {
    id: `segment-${state.nextId}`,
    revision: 1,
    lane: "translation",
    speaker: source.speaker,
    language: request.targetLanguage,
    text,
    externalTranslation: { sessionId, sourceSegmentId, revision },
  };
  const confirmed = [...state.confirmed, translation];
  const dropped = Math.max(
    0,
    confirmed.length - (state.historyLimit ?? HISTORY_LIMIT),
  );
  return {
    ...state,
    confirmed: confirmed.slice(dropped),
    nextId: state.nextId + 1,
    droppedSegments: state.droppedSegments + dropped,
  };
}

export function renameSpeaker(
  state: CaptionState,
  speaker: string,
  name: string,
): CaptionState {
  const trimmed = name.trim();
  if (trimmed)
    return {
      ...state,
      speakerNames: { ...state.speakerNames, [speaker]: trimmed },
    };
  const speakerNames = { ...state.speakerNames };
  delete speakerNames[speaker];
  return { ...state, speakerNames };
}

export function speakerLabel(state: CaptionState, speaker?: string): string {
  if (!speaker) return "说话人未识别";
  return Object.prototype.hasOwnProperty.call(state.speakerNames, speaker)
    ? state.speakerNames[speaker]
    : `说话人 ${speaker}`;
}

export function exportCaptions(state: CaptionState): string {
  const sections = (["original", "translation"] as const).map((lane) => {
    const lines = state.confirmed
      .filter((segment) => segment.lane === lane && segment.text.trim())
      .map(
        (segment) =>
          `${speakerLabel(state, segment.speaker)}  ${segment.text.trim()}`,
      );
    if (lines.length === 0) return "";
    return [lane === "original" ? "原文" : "译文", ...lines].join("\n");
  });
  if (state.droppedSegments)
    sections.unshift(
      `本次导出仅含保留的字幕。较早的 ${state.droppedSegments} 段已移除。`,
    );
  return sections.filter(Boolean).join("\n\n");
}

const demoUtterances = [
  {
    speaker: "1",
    start: 1200,
    end: 4700,
    original:
      "The fuzzer found a crash in Clang while instantiating a constrained template.",
    translation: "模糊测试器发现 Clang 在实例化带约束的模板时崩溃。",
  },
  {
    speaker: "2",
    start: 5500,
    end: 8900,
    original:
      "Can you reduce the test case and check whether the stack trace points to Sema?",
    translation: "能否缩减测试用例，并检查堆栈回溯是否指向 Sema？",
  },
  {
    speaker: "1",
    start: 9800,
    end: 13100,
    original:
      "Yes. The reduced case still crashes when Sema substitutes the template argument.",
    translation: "可以。缩减后的用例在 Sema 替换模板实参时仍会崩溃。",
  },
  {
    speaker: "2",
    start: 13900,
    end: 17400,
    original:
      "Let’s add the reproducer to the LLVM issue and link to the Clang regression test.",
    translation: "我们把复现用例加到 LLVM issue，并附上 Clang 回归测试的链接。",
  },
];

export const demoTokenBatches: TranscriptToken[][] = [
  ...demoUtterances.flatMap(
    ({ speaker, start, end, original, translation }): TranscriptToken[][] => {
      const source: TranscriptToken = {
        text: original,
        is_final: true,
        speaker,
        start_ms: start,
        end_ms: end,
        language: "en",
        translation_status: "original",
      };
      const target: TranscriptToken = {
        text: translation,
        is_final: true,
        speaker,
        language: "zh",
        source_language: "en",
        translation_status: "translation",
      };
      return [
        [
          {
            ...source,
            text: original.slice(0, Math.ceil(original.length / 2)),
            is_final: false,
          },
        ],
        [
          source,
          {
            ...target,
            text: translation.slice(0, Math.ceil(translation.length / 2)),
            is_final: false,
          },
        ],
        [target],
      ];
    },
  ),
  [
    {
      text: "I’ll also check whether",
      is_final: false,
      speaker: "1",
      start_ms: 18300,
      language: "en",
      translation_status: "original",
    },
    {
      text: "我也会检查",
      is_final: false,
      speaker: "1",
      language: "zh",
      source_language: "en",
      translation_status: "translation",
    },
  ],
];

export function createDemoCaptions(): CaptionState {
  return demoTokenBatches.reduce(applyCaptionTokens, createCaptionState());
}

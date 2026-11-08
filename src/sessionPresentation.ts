import type { CaptionSegment, CaptionState } from "./captions";

export type SessionLine = {
  segment: CaptionSegment;
  draft: boolean;
  sourceId?: string;
};

export function selectSessionTranscript(
  captions: CaptionState,
  targetLanguage: string,
) {
  const entries = [
    ...captions.confirmed.map((segment) => ({ segment, draft: false })),
    ...captions.drafts.map((segment) => ({ segment, draft: true })),
  ];
  const originals: SessionLine[] = entries.filter(
    ({ segment }) => segment.lane === "original",
  );
  const sourceById = new Map(
    originals.map((line, index) => [line.segment.id, index]),
  );
  const linked = new Map(
    entries
      .filter(({ segment }) => segment.lane === "translation")
      .map((line) => {
        const key = line.segment.externalTranslation;
        const sourceId =
          key?.sessionId === captions.sessionId
            ? key.sourceSegmentId
            : !key
              ? originals.find(
                  ({ segment }) =>
                    segment.providerItemId &&
                    segment.providerItemId === line.segment.providerItemId,
                )?.segment.id
              : undefined;
        const original = originals.find(
          (entry) => entry.segment.id === sourceId,
        )?.segment;
        return [
          line.segment.id,
          {
            ...line,
            sourceId,
            segment: {
              ...line.segment,
              speaker: line.segment.speaker ?? original?.speaker,
              startMs: line.segment.startMs ?? original?.startMs,
            },
          },
        ];
      }),
  );
  const linkedSources = new Set(
    [...linked.values()].map((line) => line.sourceId),
  );
  const translations: SessionLine[] = entries.flatMap<SessionLine>((line) => {
    const translation = linked.get(line.segment.id);
    if (translation) return [translation];
    // Providers can send only original text for speech already in the target language.
    return line.segment.lane === "original" &&
      line.segment.language === targetLanguage &&
      !linkedSources.has(line.segment.id)
      ? [{ ...line, sourceId: line.segment.id }]
      : [];
  });
  // Preserve independent provider lane order when item-level alignment is unavailable.
  if (
    translations.every(
      (line) => line.sourceId != null && sourceById.has(line.sourceId),
    )
  ) {
    translations.sort(
      (a, b) => sourceById.get(a.sourceId!)! - sourceById.get(b.sourceId!)!,
    );
  }
  return { originals, translations };
}

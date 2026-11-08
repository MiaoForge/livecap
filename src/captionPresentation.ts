import type { CaptionSegment, CaptionState } from "./captions";

export interface CaptionDisplay {
  segment: CaptionSegment;
  draft: boolean;
}

interface LiveCaptionPresentation {
  target?: CaptionDisplay;
  matchedSource?: CaptionDisplay;
  latestSource?: CaptionDisplay;
  sourceInProgress?: CaptionDisplay;
}

export function selectLiveCaptions(
  captions: CaptionState,
  targetLanguage?: string,
): LiveCaptionPresentation {
  const entries: CaptionDisplay[] = [
    ...captions.confirmed.map((segment) => ({ segment, draft: false })),
    ...captions.drafts.map((segment) => ({ segment, draft: true })),
  ];
  const sources = entries.filter(({ segment }) => segment.lane === "original");
  const latestSource = sources[sources.length - 1];
  // A provider may emit only original text when speech is already in the target language.
  if (targetLanguage && latestSource?.segment.language === targetLanguage) {
    return { target: latestSource, matchedSource: latestSource, latestSource };
  }
  const targets = entries.filter(
    ({ segment }) => segment.lane === "translation",
  );

  // Compare linked translations by source order. A provisional translation
  // for an older turn can outlive a confirmed translation for a newer one.
  const confirmedBySource = new Map<string, CaptionDisplay>();
  const draftsByItem = new Map<string, CaptionDisplay>();
  for (const target of targets) {
    const { externalTranslation, providerItemId } = target.segment;
    if (externalTranslation?.sessionId === captions.sessionId) {
      confirmedBySource.set(externalTranslation.sourceSegmentId, target);
    } else if (target.draft && providerItemId) {
      draftsByItem.set(providerItemId, target);
    }
  }
  for (let index = sources.length - 1; index >= 0; index -= 1) {
    const matchedSource = sources[index];
    const { id, providerItemId } = matchedSource.segment;
    const target =
      confirmedBySource.get(id) ??
      (providerItemId ? draftsByItem.get(providerItemId) : undefined);
    if (target) {
      return {
        target,
        matchedSource,
        latestSource,
        sourceInProgress:
          index === sources.length - 1 ? undefined : latestSource,
      };
    }
  }

  // Soniox's separate lanes carry no item-level alignment.
  return { target: targets[targets.length - 1], latestSource };
}

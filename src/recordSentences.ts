/// <reference lib="es2022.intl" />

const sentences = new Intl.Segmenter(undefined, { granularity: "sentence" });

export function timedSentences(text: string, startMs?: number, endMs?: number) {
  const parts = Array.from(sentences.segment(text), (part) => part.segment);
  if (parts.length === 0) return [];
  const total = parts.reduce((length, part) => length + part.trim().length, 0);
  const duration = startMs != null && endMs != null ? endMs - startMs : 0;
  let offset = 0;
  return parts.map((part) => {
    const from = offset;
    offset += part.trim().length;
    return {
      text: part,
      startMs:
        startMs != null && duration > 0 && total > 0
          ? startMs + (duration * from) / total
          : undefined,
      endMs:
        startMs != null && duration > 0 && total > 0
          ? startMs + (duration * offset) / total
          : undefined,
    };
  });
}

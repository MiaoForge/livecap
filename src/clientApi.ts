import { invoke, isTauri } from "@tauri-apps/api/core";
import type { TranscriptFrame } from "./captions";
export const desktop = isTauri();
export type Provider = "openai" | "soniox";
export type Settings = {
  terms: string[];
  model: string;
  provider: Provider;
  fontSize: number;
  alwaysOnTop: boolean;
  targetLanguage: string;
  meetingMicrophone: boolean;
  captionMode: "both" | "translation" | "original";
  translationFirst: boolean;
  captionTheme: "dark" | "light";
  captionOpacity: number;
  onboardingComplete: boolean;
  autoLearnMeetingTerms: boolean;
  autoGenerateMeetingNotes: boolean;
  liveMeetingSummary: boolean;
};
export type Usage = {
  session_id: string;
  provider: Provider;
  audio_ms: number;
  input_tokens: number;
  output_tokens: number;
  estimated_usd: number | null;
  transcription_usd: number | null;
  translation_usd: number | null;
  analysis_usd?: number | null;
  incomplete: boolean;
};
export type RecordSummary = {
  id: number;
  title: string;
  createdAt: string;
  durationMs: number;
  provider: Provider | null;
  status: string;
  audioAvailable: boolean;
  error: string | null;
  usage: Usage | null;
};
export type RecordDiagram = {
  nodes: { id: string; label: string; startMs: number | null }[];
  edges: { from: string; to: string; label: string }[];
};
export type TimelinePoint = {
  startMs: number | null;
  title: string;
  summary: string;
  details: string[];
};
export type RecordDetail = RecordSummary & {
  notes: string;
  targetLanguage?: string | null;
  timeline?: TimelinePoint[];
  noteSources?: (number | null)[];
  diagram?: RecordDiagram | null;
  transcriptEvents: TranscriptFrame[];
  legacyContent: string | null;
  transcriptText?: string;
  segmentEdits?: Record<string, { text: string; speaker?: string }>;
};
export type LiveSummary = {
  sessionId: string;
  phase: "waiting" | "updating" | "ready" | "error" | "unavailable" | "stopped";
  revision: number;
  error: string | null;
  content: {
    title: string;
    notes: string;
    timeline: TimelinePoint[];
    diagram: RecordDiagram | null;
    sourceTimes: (number | null)[];
    coveredThroughMs: number | null;
    truncated: boolean;
  } | null;
};
export type Knowledge = {
  domain: string;
  focus: string;
  enabled: boolean;
  updatedAt: string;
  entries: {
    term: string;
    aliases?: string[];
    coverageArea?: string;
    meaning: string;
    translation: string;
    sourceUrls: string[];
  }[];
  sources: { title: string; url: string }[];
  usage: KnowledgeUsage;
};
export type KnowledgeLibrary = { id: string; cache: Knowledge };
export type KnowledgeProgress = {
  stage:
    | "planning"
    | "researching"
    | "supplementing"
    | "reading-documents"
    | "extracting-documents"
    | "extracting"
    | "saving";
  area: string | null;
  completed: number;
  total: number;
};
export const defaults: Settings = {
  terms: [],
  model: "stt-rt-v6",
  provider: "openai",
  fontSize: 24,
  alwaysOnTop: true,
  targetLanguage: "zh",
  meetingMicrophone: true,
  captionMode: "both",
  translationFirst: true,
  captionTheme: "dark",
  captionOpacity: 0.9,
  onboardingComplete: false,
  autoLearnMeetingTerms: false,
  autoGenerateMeetingNotes: true,
  liveMeetingSummary: true,
};
export function api<T>(
  command: string,
  args?: Record<string, unknown>,
): Promise<T> {
  if (!desktop)
    return Promise.reject(new Error("请在桌面客户端中使用此功能。"));
  return invoke<T>(command, args);
}
export const money = (v: number | null | undefined) =>
  v == null ? "—" : "$" + v.toFixed(v > 0 && v < 0.01 ? 4 : 2);
export const time = (ms: number) =>
  `${Math.floor(ms / 60000)
    .toString()
    .padStart(2, "0")}:${Math.floor((ms / 1000) % 60)
    .toString()
    .padStart(2, "0")}`;
export const date = (s: string) =>
  new Date(s).toLocaleString("zh-CN", {
    month: "2-digit",
    day: "2-digit",
    hour: "2-digit",
    minute: "2-digit",
  });

export type UsageEntry = {
  id: string;
  recordId: number | null;
  mode: "subtitle" | "meeting";
  createdAt: string;
  usage: Usage;
};

export type KnowledgeUsage = {
  inputTokens: number;
  cachedInputTokens: number;
  outputTokens: number;
  webSearchCalls: number;
  estimatedUsd: number | null;
  incomplete: boolean;
};
export type KnowledgeUsageEntry = {
  id: number;
  domain: string;
  createdAt: string;
  usage: KnowledgeUsage;
};

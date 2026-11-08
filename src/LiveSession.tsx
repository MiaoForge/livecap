import { useMemo, useState } from "react";
import {
  ArrowDown,
  AudioLines,
  Captions,
  Clock3,
  Languages,
  LoaderCircle,
  Pause,
  Play,
  Sparkles,
  Square,
} from "lucide-react";
import type { CaptionState } from "./captions";
import { money, time } from "./clientApi";
import type { LiveSummary, Usage } from "./clientApi";
import { useTranscriptFollow } from "./useTranscriptFollow";
import { selectSessionTranscript } from "./sessionPresentation";
import type { SessionLine } from "./sessionPresentation";
import { LiveSummaryPanel } from "./LiveSummaryPanel";
import { languageName } from "./ProductSettings";

type Props = {
  captions: CaptionState;
  mode: "meeting" | "subtitle";
  status: string;
  topic: string;
  targetLanguage: string;
  summaryEnabled: boolean;
  summary: LiveSummary | null;
  usage: Usage | null;
  busy: boolean;
  onPause: () => void;
  onStop: () => void;
  onOverlay: () => void;
  onSettings: () => void;
};

function Speaker({
  line,
  captions,
}: {
  line: SessionLine;
  captions: CaptionState;
}) {
  return (
    <span className="session-speaker">
      {line.segment.speaker
        ? captions.speakerNames[line.segment.speaker] ||
          `说话人 ${line.segment.speaker}`
        : "发言"}
    </span>
  );
}

export function LiveSession({
  captions,
  mode,
  status,
  topic,
  targetLanguage,
  summaryEnabled,
  summary,
  usage,
  busy,
  onPause,
  onStop,
  onOverlay,
  onSettings,
}: Props) {
  const [tab, setTab] = useState<"translation" | "summary">("translation");
  const [highlight, setHighlight] = useState<string | null>(null);
  const [jumpNotice, setJumpNotice] = useState("");
  const { originals, translations } = useMemo(
    () => selectSessionTranscript(captions, targetLanguage),
    [captions, targetLanguage],
  );
  const originalFollow = useTranscriptFollow(originals);
  const translationFollow = useTranscriptFollow(
    translations,
    tab === "translation",
  );
  const originalsPane = originalFollow.pane;
  const active = ["connecting", "listening", "paused", "stopping"].includes(
    status,
  );
  const paused = status === "paused";
  const stateLabel =
    (
      {
        connecting: "正在连接",
        listening: mode === "meeting" ? "录音中" : "字幕进行中",
        paused: "已暂停",
        stopping: "正在保存",
        idle: mode === "meeting" ? "已归档" : "已结束",
        error: "会话中断",
      } as Record<string, string>
    )[status] || status;
  const updating = summary?.phase === "updating";
  function jumpToOriginal(id?: string, ms?: number) {
    let line = id
      ? originals.find((entry) => entry.segment.id === id)
      : undefined;
    if (!line && ms != null) {
      line =
        originals.find(
          (entry) =>
            entry.segment.startMs != null &&
            entry.segment.startMs <= ms &&
            (entry.segment.endMs ?? entry.segment.startMs) >= ms,
        ) ??
        originals.find(
          (entry) =>
            entry.segment.startMs != null && entry.segment.startMs >= ms,
        );
      const firstTime = originals.find((entry) => entry.segment.startMs != null)
        ?.segment.startMs;
      if (firstTime != null && ms < firstTime && captions.droppedSegments > 0)
        line = undefined;
    }
    if (!line) {
      setJumpNotice(
        mode === "meeting"
          ? "这段原文已超出实时显示范围，可在会后记录中查看。"
          : "这段原文已超出本次字幕的显示范围。",
      );
      return;
    }
    const node = originalsPane.current?.querySelector<HTMLElement>(
      `[data-session-source="${originals.indexOf(line)}"]`,
    );
    if (node) {
      originalFollow.pause();
      setHighlight(line.segment.id);
      setJumpNotice("");
      const pane = originalsPane.current!;
      pane.scrollTo({
        top:
          pane.scrollTop +
          node.getBoundingClientRect().top -
          pane.getBoundingClientRect().top -
          16,
        behavior: "smooth",
      });
      if (window.matchMedia("(max-width: 760px)").matches)
        pane.scrollIntoView({ block: "center", behavior: "smooth" });
    }
  }
  return (
    <div className="live-session">
      <header className="live-session-heading">
        <div className="live-session-title">
          <span className="live-session-eyebrow">
            {mode === "meeting" ? "会议工作台" : "实时翻译"}
          </span>
          <h1>{topic || (mode === "meeting" ? "当前会议" : "当前会话")}</h1>
          <div className="live-session-meta">
            <span className={`session-state ${status}`}>
              <i />
              {stateLabel}
            </span>
            <span>
              <Clock3 />
              {time(usage?.audio_ms ?? 0)}
            </span>
            <span>
              {mode === "meeting" ? "录音与文字自动保存" : "本次内容不保存"}
            </span>
            <span>预计费用 {money(usage?.estimated_usd)}</span>
          </div>
        </div>
        <div className="live-session-actions">
          <button onClick={onOverlay}>
            <Captions />
            字幕窗
          </button>
          {mode === "meeting" && ["listening", "paused"].includes(status) && (
            <button disabled={busy} onClick={onPause}>
              {paused ? <Play /> : <Pause />}
              {paused ? "继续" : "暂停"}
            </button>
          )}
          <button
            className="session-stop"
            disabled={!active || busy || status === "stopping"}
            onClick={onStop}
          >
            <Square />
            {status === "stopping"
              ? "正在保存…"
              : mode === "meeting"
                ? "结束会议"
                : "结束会话"}
          </button>
        </div>
      </header>
      {paused && (
        <p className="session-pause-notice" role="status">
          会议已暂停，新音频不会录制或发送。已录入的内容仍可能继续翻译和整理。
          {usage?.provider === "soniox" && "Soniox 在线连接仍按连接时长计费。"}
        </p>
      )}
      <div className="live-session-columns">
        <section
          className="session-reading"
          aria-label={mode === "meeting" ? "翻译与会议摘要" : "实时译文"}
        >
          <header className="session-pane-heading">
            <div
              className="session-reading-tabs"
              role="tablist"
              aria-label="阅读内容"
              onKeyDown={(event) => {
                if (
                  mode !== "meeting" ||
                  !["ArrowLeft", "ArrowRight", "Home", "End"].includes(
                    event.key,
                  )
                )
                  return;
                event.preventDefault();
                const next =
                  event.key === "Home"
                    ? "translation"
                    : event.key === "End"
                      ? "summary"
                      : tab === "translation"
                        ? "summary"
                        : "translation";
                setTab(next);
                document.getElementById(`session-${next}-tab`)?.focus();
              }}
            >
              <button
                id="session-translation-tab"
                role="tab"
                tabIndex={tab === "translation" ? 0 : -1}
                aria-selected={tab === "translation"}
                aria-controls="session-translation-panel"
                onClick={() => setTab("translation")}
              >
                <Languages />
                实时翻译
              </button>
              {mode === "meeting" && (
                <button
                  id="session-summary-tab"
                  role="tab"
                  tabIndex={tab === "summary" ? 0 : -1}
                  aria-selected={tab === "summary"}
                  aria-controls="session-summary-panel"
                  onClick={() => setTab("summary")}
                >
                  <Sparkles />
                  图文摘要
                  {updating && <LoaderCircle className="session-spin" />}
                </button>
              )}
            </div>
            <span className="session-pane-language">
              {languageName(targetLanguage)}
            </span>
          </header>
          <div hidden={tab !== "translation"} className="session-tab-container">
            <div
              id="session-translation-panel"
              role="tabpanel"
              aria-labelledby="session-translation-tab"
              className="session-reading-body"
            >
              <div
                className="session-feed"
                ref={translationFollow.pane}
                onScroll={translationFollow.onScroll}
                onWheel={translationFollow.onWheel}
                onKeyDown={translationFollow.onKeyDown}
                onTouchStart={translationFollow.onTouchStart}
                onTouchMove={translationFollow.onTouchMove}
                tabIndex={0}
                aria-label="译文滚动区域"
              >
                <div
                  ref={translationFollow.body}
                  className="session-feed-content"
                >
                  {!translations.length && (
                    <div className="session-empty">
                      <Languages />
                      <h2>让对话自然发生</h2>
                      <p>
                        {originals.length
                          ? "已收到原文，译文会陆续显示在这里。"
                          : "发言开始后，译文会按段落显示在这里。"}
                      </p>
                    </div>
                  )}
                  {translations.map((line) => (
                    <article
                      key={line.segment.id}
                      className={`session-speech translation ${line.draft ? "draft" : ""}`}
                    >
                      <header>
                        <Speaker line={line} captions={captions} />
                        {line.segment.startMs != null && (
                          <span>{time(line.segment.startMs)}</span>
                        )}
                        {line.sourceId && (
                          <button
                            onClick={() => jumpToOriginal(line.sourceId)}
                            aria-label="定位这段译文的原文"
                          >
                            原文 ↗
                          </button>
                        )}
                        {line.draft && (
                          <span className="session-draft-label">翻译中</span>
                        )}
                      </header>
                      <p>{line.segment.text}</p>
                    </article>
                  ))}
                </div>
              </div>
              <footer className="session-pane-footer">
                <span>
                  {translations.length ? "译文随发言更新" : "等待发言"}
                </span>
                <button
                  onClick={translationFollow.resume}
                  disabled={translationFollow.following}
                >
                  <ArrowDown />
                  {translationFollow.following ? "跟随最新" : "回到最新"}
                </button>
              </footer>
            </div>
          </div>
          {mode === "meeting" && (
            <div hidden={tab !== "summary"} className="session-tab-container">
              <LiveSummaryPanel
                summary={summary}
                summaryEnabled={summaryEnabled}
                active={active}
                onJump={(ms) => jumpToOriginal(undefined, ms)}
                onSettings={onSettings}
              />
            </div>
          )}
        </section>
        <section className="session-original" aria-label="实时原文">
          <header className="session-pane-heading">
            <div>
              <AudioLines />
              <h2>实时原文</h2>
            </div>
            <span className="session-pane-language">自动识别</span>
          </header>
          <div
            className="session-feed"
            ref={originalsPane}
            onScroll={originalFollow.onScroll}
            onWheel={originalFollow.onWheel}
            onKeyDown={originalFollow.onKeyDown}
            onTouchStart={originalFollow.onTouchStart}
            onTouchMove={originalFollow.onTouchMove}
            tabIndex={0}
            aria-label="原文滚动区域"
          >
            <div ref={originalFollow.body} className="session-feed-content">
              {!originals.length && (
                <div className="session-empty">
                  <AudioLines />
                  <h2>
                    {status === "connecting"
                      ? "正在连接转写服务"
                      : paused
                        ? "等待继续会议"
                        : status === "error"
                          ? "转写已中断"
                          : active
                            ? "正在聆听"
                            : "本场没有转写内容"}
                  </h2>
                  <p>原文会持续录入，可随时向上回看。</p>
                </div>
              )}
              {originals.map((line, index) => (
                <article
                  data-session-source={index}
                  key={line.segment.id}
                  className={`session-speech original ${line.draft ? "draft" : ""} ${highlight === line.segment.id ? "highlighted" : ""}`}
                >
                  <header>
                    <Speaker line={line} captions={captions} />
                    {line.segment.startMs != null && (
                      <span>{time(line.segment.startMs)}</span>
                    )}
                    {line.draft && (
                      <span className="session-draft-label">录入中</span>
                    )}
                  </header>
                  <p>{line.segment.text}</p>
                </article>
              ))}
            </div>
          </div>
          {jumpNotice && (
            <p className="session-jump-notice" role="status">
              {jumpNotice}
            </p>
          )}
          <footer className="session-pane-footer">
            <span>
              {captions.droppedSegments
                ? mode === "meeting"
                  ? "较早内容请在会后记录查看"
                  : "仅显示最近的原文"
                : mode === "meeting"
                  ? "确认后的原文用于整理摘要"
                  : "原文随发言持续录入"}
            </span>
            <button
              onClick={() => {
                originalFollow.resume();
                setHighlight(null);
              }}
              disabled={originalFollow.following}
            >
              <ArrowDown />
              {originalFollow.following ? "跟随最新" : "回到最新"}
            </button>
          </footer>
        </section>
      </div>
    </div>
  );
}

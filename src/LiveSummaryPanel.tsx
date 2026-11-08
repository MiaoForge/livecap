import { ErrorNotice } from "./Notifications";
import { Check, FileText, LoaderCircle, Sparkles } from "lucide-react";
import { time } from "./clientApi";
import type { LiveSummary } from "./clientApi";
import { NotesDocument } from "./NotesDocument";
import { MeetingDiagram } from "./MeetingDiagram";

export function LiveSummaryPanel({
  summary,
  summaryEnabled,
  active,
  onJump,
  onSettings,
}: {
  summary: LiveSummary | null;
  summaryEnabled: boolean;
  active: boolean;
  onJump: (ms: number) => void;
  onSettings: () => void;
}) {
  const content = summary?.content;
  const updating = summary?.phase === "updating";
  return (
    <div
      id="session-summary-panel"
      role="tabpanel"
      aria-labelledby="session-summary-tab"
      className="session-summary-body"
    >
      <div className="session-summary-status" role="status">
        {updating ? (
          <LoaderCircle className="session-spin" />
        ) : content ? (
          <Check />
        ) : (
          <Sparkles />
        )}
        <span>
          {!summaryEnabled
            ? "会中自动摘要已关闭"
            : summary?.phase === "unavailable"
              ? "会中摘要暂不可用"
              : updating
                ? "正在合并新的发言…"
                : content
                  ? `已整理${content.coveredThroughMs != null ? "至 " + time(content.coveredThroughMs) : "当前发言"}${active && summary?.phase !== "stopped" ? " · 自动更新" : ""}`
                  : active && summary?.phase !== "stopped"
                    ? "积累足够发言后，自动生成第一版摘要"
                    : "本场尚未生成会中摘要"}
        </span>
      </div>
      {summary?.error && (
        <div className="session-summary-error">
          <ErrorNotice message={summary.error} kind="warning" />
          {summary.phase === "unavailable" ? (
            <button onClick={onSettings}>连接服务 →</button>
          ) : (
            <small>
              {summary.phase === "stopped"
                ? "会后可在记录中重新生成摘要。"
                : "保留上一版摘要，稍后自动重试。"}
            </small>
          )}
        </div>
      )}
      {content ? (
        <div className="session-summary-document">
          <div className="session-summary-document-heading">
            <span>会中摘要 · 第 {summary?.revision} 版</span>
            <h2>{content.title}</h2>
            <p>随对话逐步整理，结束后自动归档到会议记录。</p>
          </div>
          {content.truncated && (
            <p className="session-summary-coverage" role="status">
              部分发言超出本次整理范围，完整原文仍会保存在会议记录中。
            </p>
          )}
          {content.diagram && <MeetingDiagram diagram={content.diagram} />}
          <NotesDocument
            text={content.notes}
            sourceTimes={content.sourceTimes}
            seek={onJump}
            seekLabel="定位原文"
          />
          {!!content.timeline.length && (
            <section
              className="session-summary-outline"
              aria-label="要点与原文"
            >
              <h3>要点与原文</h3>
              {content.timeline.map((point, index) => (
                <article key={index}>
                  <button
                    disabled={point.startMs == null}
                    onClick={() => onJump(point.startMs!)}
                  >
                    <span>
                      {point.startMs == null ? "—" : time(point.startMs)}
                    </span>
                    {point.title}
                    <span>↗</span>
                  </button>
                  <p>{point.summary}</p>
                </article>
              ))}
            </section>
          )}
        </div>
      ) : (
        <div className="session-empty session-summary-empty">
          <FileText />
          <h2>{summaryEnabled ? "一边开会，一边理清思路" : "专注实时翻译"}</h2>
          <p>
            {summaryEnabled
              ? "确认后的原文会逐步整理为核心要点、内容关系图和带来源的摘要。"
              : "可在设置中开启会中自动摘要，下次会议生效。"}
          </p>
          <div className="session-summary-steps">
            <span>确认原文</span>
            <span>→</span>
            <span>提炼要点</span>
            <span>→</span>
            <span>更新摘要</span>
          </div>
        </div>
      )}
    </div>
  );
}

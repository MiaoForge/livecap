import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import { createCaptionState } from "./captions";
import type { LiveSummary } from "./clientApi";
import { LiveSession } from "./LiveSession";
import { LiveSummaryPanel } from "./LiveSummaryPanel";
import { NotesDocument } from "./NotesDocument";

const summary: LiveSummary = {
  sessionId: "meeting",
  phase: "updating",
  revision: 1,
  error: null,
  content: {
    title: "Compiler pipeline",
    notes: "## 要点\n编译流程 [S1]",
    timeline: [
      { startMs: 1000, title: "Parser", summary: "Build the AST", details: [] },
    ],
    diagram: null,
    sourceTimes: [1000],
    coveredThroughMs: 12_000,
    truncated: false,
  },
};
const noop = () => {};

describe("live session workspace", () => {
  it("provides separate translation and original panes with real session data", () => {
    const captions = createCaptionState();
    captions.confirmed = [
      {
        id: "one",
        revision: 1,
        lane: "original",
        text: "Build the AST.",
        startMs: 1000,
        speaker: "1",
      },
      {
        id: "two",
        revision: 1,
        lane: "translation",
        text: "构建抽象语法树。",
        startMs: 1000,
      },
    ];
    const html = renderToStaticMarkup(
      <LiveSession
        captions={captions}
        mode="meeting"
        status="listening"
        topic="Compiler pipeline"
        targetLanguage="zh"
        summaryEnabled
        summary={null}
        usage={null}
        busy={false}
        onPause={noop}
        onStop={noop}
        onOverlay={noop}
        onSettings={noop}
      />,
    );
    expect(html).toContain('aria-label="翻译与会议摘要"');
    expect(html).toContain('aria-label="实时原文"');
    expect(html).toContain("构建抽象语法树。");
    expect(html).toContain("Build the AST.");
    for (const panel of ["translation", "summary"]) {
      expect(html).toContain(`aria-controls="session-${panel}-panel"`);
      expect(html).toContain(`id="session-${panel}-panel"`);
    }
    expect(html).toContain("说话人 1");
    expect(html).toContain("结束会议");
  });

  it("keeps subtitle mode transient and does not offer meeting-only summaries", () => {
    const captions = createCaptionState();
    captions.droppedSegments = 1;
    const html = renderToStaticMarkup(
      <LiveSession
        captions={captions}
        mode="subtitle"
        status="listening"
        topic=""
        targetLanguage="zh"
        summaryEnabled
        summary={null}
        usage={null}
        busy={false}
        onPause={noop}
        onStop={noop}
        onOverlay={noop}
        onSettings={noop}
      />,
    );
    expect(html).toContain("本次内容不保存");
    expect(html).toContain("仅显示最近的原文");
    expect(html).not.toContain("图文摘要");
    expect(html).not.toContain("会后记录");
  });

  it("retains the previous summary while a newer one is generated", () => {
    const html = renderToStaticMarkup(
      <LiveSummaryPanel
        summary={summary}
        summaryEnabled
        active
        onJump={noop}
        onSettings={noop}
      />,
    );
    expect(html).toContain("正在合并新的发言");
    expect(html).toContain("Compiler pipeline");
    expect(html).toContain("第 1 版");
    expect(html).toContain('aria-label="定位原文 00:01（来源 S1）"');
    expect(html).not.toContain("回听");
  });

  it("explains unavailable and stopped summaries without promising retries after the meeting", () => {
    const unavailable = renderToStaticMarkup(
      <LiveSummaryPanel
        summary={{
          ...summary,
          phase: "unavailable",
          error: "请连接 OpenAI。",
          content: null,
        }}
        summaryEnabled
        active
        onJump={noop}
        onSettings={noop}
      />,
    );
    expect(unavailable).toContain("会中摘要暂不可用");
    expect(unavailable).toContain("连接服务");
    const stopped = renderToStaticMarkup(
      <LiveSummaryPanel
        summary={{ ...summary, phase: "stopped", error: "请求失败。" }}
        summaryEnabled
        active={false}
        onJump={noop}
        onSettings={noop}
      />,
    );
    expect(stopped).toContain("Compiler pipeline");
    expect(stopped).toContain("会后可在记录中重新生成摘要");
    expect(stopped).not.toContain("自动重试");
    expect(stopped).not.toContain("自动更新");
  });

  it("keeps the existing playback wording for archived note citations", () => {
    const html = renderToStaticMarkup(
      <NotesDocument text="原文 [S1]" sourceTimes={[1000]} seek={noop} />,
    );
    expect(html).toContain('aria-label="回听 00:01（来源 S1）"');
  });
});

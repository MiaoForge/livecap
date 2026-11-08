import { useEffect, useRef, useState, useMemo } from "react";
import type { ReactNode } from "react";
import { listen, emit } from "@tauri-apps/api/event";
import { WebviewWindow } from "@tauri-apps/api/webviewWindow";
import { getCurrentWindow, currentMonitor } from "@tauri-apps/api/window";
import { PhysicalPosition } from "@tauri-apps/api/dpi";
import {
  ArrowLeft,
  ArrowUpRight,
  AudioLines,
  BookOpen,
  Captions as CaptionsIcon,
  ChartPie,
  Copy,
  Download,
  FileText,
  Home,
  Pause,
  Pencil,
  Play,
  Radio,
  Settings2,
  Sparkles,
  UsersRound,
  MessageCircle,
  X,
  GripHorizontal,
  Square,
} from "lucide-react";
import { api, desktop, defaults, money, time, date } from "./clientApi";
import type {
  Settings,
  Usage,
  UsageEntry,
  RecordSummary,
  RecordDetail,
  Knowledge,
  KnowledgeUsage,
  KnowledgeUsageEntry,
} from "./clientApi";
import {
  applyCaptionFrame,
  createCaptionState,
  exportCaptions,
} from "./captions";
import type { CaptionState, TranscriptFrame } from "./captions";
import { selectLiveCaptions } from "./captionPresentation";
import "./Client.css";
import { NotesDocument } from "./NotesDocument";
import { MeetingDiagram } from "./MeetingDiagram";
import { timedSentences } from "./recordSentences";
import { ProductSettings, languageName } from "./ProductSettings";
import { ProductKnowledge } from "./ProductKnowledge";
import type { LearnedTerm } from "./ProductKnowledge";

type Status = { state: string; message?: string };
type SummaryStatus = { running: boolean; error?: string | null };
type Snapshot = {
  captions: CaptionState;
  status: Status;
  settings: Settings;
  mode: "subtitle" | "meeting";
};
const names: Record<string, string> = {
  home: "首页",
  records: "记录",
  knowledge: "知识空间",
  usage: "用量",
  settings: "设置",
  session: "当前会话",
};
const navIcons: Record<string, ReactNode> = {
  home: <Home />,
  records: <FileText />,
  knowledge: <BookOpen />,
  session: <Radio />,
  usage: <ChartPie />,
};
function Dialog({
  title,
  children,
  close,
}: {
  title: string;
  children: ReactNode;
  close: () => void;
}) {
  const ref = useRef<HTMLDialogElement>(null);
  useEffect(() => {
    ref.current?.showModal();
  }, []);
  return (
    <dialog
      ref={ref}
      className="native-dialog"
      onCancel={(e) => {
        e.preventDefault();
        close();
      }}
    >
      <div className="heading-row">
        <h2>{title}</h2>
        <button onClick={close}>×</button>
      </div>
      {children}
    </dialog>
  );
}
function Captions({ snapshot }: { snapshot: Snapshot }) {
  const container = useRef<HTMLDivElement>(null);
  const s = snapshot.settings;
  useEffect(() => {
    container.current
      ?.querySelectorAll(".native-translation,.native-original")
      .forEach((el) => {
        el.scrollTop = el.scrollHeight;
      });
  }, [snapshot.captions]);
  const live = selectLiveCaptions(snapshot.captions, s.targetLanguage);
  const original = live.matchedSource ?? live.latestSource;
  return (
    <div
      ref={container}
      className={
        "native-captions " + (!s.translationFirst ? "original-first" : "")
      }
      style={{ fontSize: s.fontSize }}
    >
      {s.captionMode !== "original" && (
        <div className="native-translation">
          {live.target?.segment.text || "等待发言…"}
        </div>
      )}
      {s.captionMode !== "translation" && (
        <div className="native-original">
          {original?.segment.text ||
            `自动识别 → ${languageName(s.targetLanguage)}`}
        </div>
      )}
      {s.captionMode !== "translation" && live.sourceInProgress && (
        <small>{live.sourceInProgress.segment.text}</small>
      )}
    </div>
  );
}
function CaptionWindow() {
  useEffect(() => {
    document.body.classList.add("caption-surface");
    return () => document.body.classList.remove("caption-surface");
  }, []);
  const [snapshot, setSnapshot] = useState<Snapshot>({
    captions: createCaptionState(),
    status: { state: "idle" },
    settings: defaults,
    mode: "subtitle",
  });
  useEffect(() => {
    const subscription = listen<Snapshot>("livecap-caption-state", (event) => {
      setSnapshot(event.payload);
      void getCurrentWindow().setAlwaysOnTop(
        event.payload.settings.alwaysOnTop,
      );
    });
    void subscription.then(() => emit("livecap-caption-ready"));
    return () => {
      void subscription.then((unlisten) => unlisten());
    };
  }, []);
  const { settings, status } = snapshot;
  const running = ["connecting", "listening", "paused", "stopping"].includes(
    status.state,
  );
  const label = (
    {
      listening: "实时字幕",
      paused: "会议已暂停",
      connecting: "正在连接",
      stopping: "正在保存",
      idle: "字幕预览",
      error: "连接中断",
    } as Record<string, string>
  )[status.state];
  return (
    <div
      className={"native-caption-window " + settings.captionTheme}
      style={{
        backgroundColor:
          settings.captionTheme === "dark"
            ? `rgba(22,27,24,${settings.captionOpacity})`
            : `rgba(250,252,247,${settings.captionOpacity})`,
      }}
    >
      <header
        onMouseDown={(event) => {
          if (
            event.button === 0 &&
            !(event.target as HTMLElement).closest("button")
          )
            void getCurrentWindow().startDragging();
        }}
      >
        <span className="caption-window-state">
          <i className={running ? "live" : ""} />
          {label}
        </span>
        <GripHorizontal className="caption-grip" size={18} aria-hidden="true" />
        <div className="caption-window-controls">
          {snapshot.mode === "meeting" &&
            ["listening", "paused"].includes(status.state) && (
              <button
                title={status.state === "paused" ? "继续会议" : "暂停会议"}
                aria-label={status.state === "paused" ? "继续会议" : "暂停会议"}
                onClick={() =>
                  void api("set_meeting_paused", {
                    paused: status.state !== "paused",
                  }).catch(console.error)
                }
              >
                {status.state === "paused" ? (
                  <Play size={13} />
                ) : (
                  <Pause size={13} />
                )}
              </button>
            )}
          {running && (
            <button
              title="结束会话"
              aria-label="结束会话"
              disabled={status.state === "stopping"}
              onClick={() => void emit("livecap-stop-request")}
            >
              <Square size={13} />
            </button>
          )}
          <button
            title="隐藏字幕窗"
            aria-label="隐藏字幕窗"
            onClick={() => void getCurrentWindow().hide()}
          >
            <X size={16} />
          </button>
        </div>
      </header>
      <Captions snapshot={snapshot} />
    </div>
  );
}
export default function Client() {
  return new URLSearchParams(location.search).get("window") === "captions" ? (
    <CaptionWindow />
  ) : (
    <Workspace />
  );
}
function Workspace() {
  const [mode, setMode] = useState<"subtitle" | "meeting">("subtitle");
  const overlayPlaced = useRef(false);
  const modeRef = useRef(mode);
  modeRef.current = mode;
  const [onboardStep, setOnboardStep] = useState(0);
  const [learned, setLearned] = useState<LearnedTerm[]>([]);
  const [meetingTopic, setMeetingTopic] = useState("");
  const [learningNotice, setLearningNotice] = useState("");
  const [usageEntries, setUsageEntries] = useState<UsageEntry[]>([]);
  const [knowledgeUsage, setKnowledgeUsage] = useState<KnowledgeUsageEntry[]>(
    [],
  );
  const [page, setPage] = useState("home"),
    [settings, setSettings] = useState(defaults),
    [keys, setKeys] = useState<Partial<Record<Settings["provider"], boolean>>>(
      {},
    );
  const [status, setStatus] = useState<Status>({ state: "idle" }),
    [captions, setCaptions] = useState(createCaptionState),
    [usage, setUsage] = useState<Usage | null>(null),
    [records, setRecords] = useState<RecordSummary[]>([]),
    [record, setRecord] = useState<RecordDetail | null>(null),
    [knowledge, setKnowledge] = useState<Knowledge | null>(null);
  const [error, setError] = useState(""),
    [busy, setBusy] = useState(false),
    [ready, setReady] = useState(false),
    [consent, setConsent] = useState(false),
    [generating, setGenerating] = useState(false),
    [query, setQuery] = useState("");
  const [snapshotRef] = useState(() => ({ current: null as Snapshot | null }));
  snapshotRef.current = { captions, status, settings, mode };
  const active = ["connecting", "listening", "paused", "stopping"].includes(
    status.state,
  );
  const openRecordId = useRef<number | null>(null);
  openRecordId.current = record?.id ?? null;
  const [summaryStatus, setSummaryStatus] = useState<
    Record<number, SummaryStatus>
  >({});
  const fail = (e: unknown) =>
    setError(String(e instanceof Error ? e.message : e));
  async function refresh() {
    const [r, u, k] = await Promise.all([
      api<RecordSummary[]>("list_records"),
      api<UsageEntry[]>("list_usage"),
      api<KnowledgeUsageEntry[]>("list_knowledge_usage"),
    ]);
    setRecords(r);
    setUsageEntries(u);
    setKnowledgeUsage(k);
  }
  useEffect(() => {
    if (!desktop) {
      setReady(true);
      return;
    }
    let disposed = false;
    const subscriptions = [
      listen<Status>("session-status", (e) => {
        const wasActive = [
          "connecting",
          "listening",
          "paused",
          "stopping",
        ].includes(snapshotRef.current?.status.state ?? "");
        setStatus(e.payload);
        if (wasActive && ["idle", "error"].includes(e.payload.state)) {
          void WebviewWindow.getByLabel("captions")
            .then((window) => window?.hide())
            .catch(fail);
        }
        if (
          ["idle", "error"].includes(e.payload.state) &&
          modeRef.current === "subtitle"
        )
          setCaptions(createCaptionState());
        if (e.payload.message) setError(e.payload.message);
        if (["idle", "error"].includes(e.payload.state))
          void refresh().catch(fail);
      }),
      listen<TranscriptFrame>("transcript", (e) =>
        setCaptions((c) => applyCaptionFrame(c, e.payload)),
      ),
      listen<Usage>("session-usage", (e) => setUsage(e.payload)),
      listen<{ candidates: LearnedTerm[] }>("session-terms", (e) =>
        setLearned(e.payload.candidates),
      ),
      listen<string>("translation-warning", (e) => setError(e.payload)),
      listen<{ id: number }>("records-updated", (event) => {
        void refresh().catch(fail);
        const id = event.payload.id;
        if (openRecordId.current === id)
          void api<RecordDetail>("get_record", { id })
            .then((next) =>
              setRecord((current) => (current?.id === id ? next : current)),
            )
            .catch(fail);
      }),
      listen<SummaryStatus & { id: number }>(
        "record-summary-status",
        (event) => {
          const { id, ...summary } = event.payload;
          setSummaryStatus((current) => ({ ...current, [id]: summary }));
        },
      ),
      listen("usage-updated", () => void refresh().catch(fail)),
      listen<string>("knowledge-learning", (e) => setLearningNotice(e.payload)),
      listen(
        "knowledge-updated",
        () =>
          void api<Knowledge | null>("load_knowledge")
            .then(setKnowledge)
            .catch(fail),
      ),
      listen(
        "livecap-stop-request",
        () => void api("stop_session").catch(fail),
      ),
      listen("livecap-caption-ready", () => {
        if (snapshotRef.current)
          void emit("livecap-caption-state", snapshotRef.current);
      }),
    ];
    void Promise.all([
      api<Settings>("load_settings"),
      api<boolean>("has_api_key", { provider: "openai" }),
      api<boolean>("has_api_key", { provider: "soniox" }),
      api<Knowledge | null>("load_knowledge"),
      api<RecordSummary[]>("list_records"),
      api<UsageEntry[]>("list_usage"),
      api<KnowledgeUsageEntry[]>("list_knowledge_usage"),
      ...subscriptions,
    ])
      .then((values) => {
        if (disposed) return;
        setSettings({ ...defaults, ...values[0] });
        setKeys({ openai: values[1], soniox: values[2] });
        setKnowledge(values[3]);
        setRecords(values[4]);
        setUsageEntries(values[5]);
        setKnowledgeUsage(values[6]);
        setReady(true);
      })
      .catch(fail);
    return () => {
      disposed = true;
      subscriptions.forEach((p) => void p.then((f) => f()));
    };
  }, []);
  useEffect(() => {
    if (desktop)
      void emit("livecap-caption-state", {
        captions,
        status,
        settings,
        mode,
      }).catch(fail);
  }, [captions, status, settings, mode]);
  async function overlay() {
    const w = await WebviewWindow.getByLabel("captions");
    if (w) {
      if (!overlayPlaced.current) {
        overlayPlaced.current = true;
        const monitor = await currentMonitor();
        if (monitor) {
          const size = await w.outerSize();
          await w.setPosition(
            new PhysicalPosition(
              monitor.position.x +
                Math.max(0, (monitor.size.width - size.width) / 2),
              monitor.position.y +
                Math.max(
                  0,
                  monitor.size.height - size.height - 65 * monitor.scaleFactor,
                ),
            ),
          );
        }
      }
      await w.show();
      await emit("livecap-caption-state", snapshotRef.current);
    }
  }
  async function start() {
    setConsent(false);
    setBusy(true);
    setError("");
    const next = createCaptionState();
    setCaptions(next);
    setUsage(null);
    try {
      await api("save_settings", { settings });
      await api("start_session", {
        consent: true,
        provider: settings.provider,
        sessionId: next.sessionId,
        mode,
        meetingTopic: mode === "meeting" ? meetingTopic.trim() : undefined,
      });
      setPage("session");
      await overlay();
    } catch (e) {
      fail(e);
    } finally {
      setBusy(false);
    }
  }
  async function pauseMeeting() {
    setBusy(true);
    try {
      await api("set_meeting_paused", { paused: status.state !== "paused" });
    } catch (error) {
      fail(error);
    } finally {
      setBusy(false);
    }
  }
  async function stop() {
    setBusy(true);
    try {
      await api("stop_session");
    } catch (e) {
      fail(e);
    } finally {
      setBusy(false);
    }
  }
  async function openRecord(id: number) {
    setBusy(true);
    try {
      setRecord(await api<RecordDetail>("get_record", { id }));
      setPage("records");
    } catch (e) {
      fail(e);
    } finally {
      setBusy(false);
    }
  }

  const nav = (p: string) => {
    setPage(p);
    setRecord(null);
    setQuery("");
  };
  async function finishOnboarding() {
    try {
      const next = { ...settings, onboardingComplete: true };
      await api("save_settings", { settings: next });
      setSettings(next);
      setPage("home");
    } catch (e) {
      fail(e);
    }
  }
  if (ready && desktop && !settings.onboardingComplete)
    return (
      <div className="product-onboarding">
        <header>
          <span className="brand">
            <span className="brand-icon">
              <i />
              <i />
              <i />
              <i />
            </span>
            livecap
          </span>
          <div className="onboard-dots">
            {[0, 1, 2, 3].map((i) => (
              <i key={i} className={i <= onboardStep ? "active" : ""} />
            ))}
          </div>
          <button
            className="ghost"
            disabled={generating}
            onClick={() => void finishOnboarding()}
          >
            稍后配置
          </button>
        </header>
        <main>
          {error && (
            <div role="alert" className="native-error">
              {error}
            </div>
          )}
          {onboardStep === 0 ? (
            <div className="onboard-welcome">
              <div className="onboard-wave">
                <i />
                <i />
                <i />
                <i />
                <i />
              </div>
              <span className="eyebrow">WELCOME TO LIVECAP</span>
              <h1>
                听懂每一句，
                <br />
                专注你关心的内容。
              </h1>
              <p>
                从一段视频，到一场会议。
                <br />
                用实时字幕理解，用会议记录回顾。
              </p>
              <button className="primary" onClick={() => setOnboardStep(1)}>
                开始配置 →
              </button>
            </div>
          ) : onboardStep === 1 ? (
            <>
              <div className="onboard-heading">
                <small>01 / 连接服务</small>
                <h1>连接你的翻译服务</h1>
                <p>使用自己的 API Key，密钥安全保存在本机。</p>
              </div>
              <ProductSettings
                initialTab="connection"
                settings={settings}
                setSettings={setSettings}
                keys={keys}
                onKeys={setKeys}
                active={false}
                preview={() => void overlay().catch(fail)}
                fail={fail}
              />
            </>
          ) : onboardStep === 2 ? (
            <ProductKnowledge
              cache={knowledge}
              onCache={setKnowledge}
              hasKey={Boolean(keys.openai)}
              active={false}
              fail={fail}
              onGenerating={setGenerating}
              openSettings={() => setOnboardStep(1)}
              learned={learned}
              settings={settings}
              onSettings={setSettings}
            />
          ) : (
            <div className="onboard-ready">
              <div className="onboard-ready-icon">✓</div>
              <h1>准备好了</h1>
              <p>
                {keys[settings.provider]
                  ? "翻译服务已配置"
                  : "你可以随时在设置中连接服务"}
                {knowledge ? " · 领域知识已就绪" : ""}
              </p>
              <div className="native-mode-picker">
                <button
                  className={mode === "subtitle" ? "selected" : ""}
                  onClick={() => setMode("subtitle")}
                >
                  <h2>字幕翻译</h2>
                  <p>视频与电影，轻松听懂</p>
                  <small>不保存音频和文字</small>
                </button>
                <button
                  className={mode === "meeting" ? "selected" : ""}
                  onClick={() => setMode("meeting")}
                >
                  <h2>会议翻译</h2>
                  <p>讨论与协作，完整回顾</p>
                  <small>自动保存录音与记录</small>
                </button>
              </div>
              <button
                className="primary"
                onClick={() => void finishOnboarding()}
              >
                进入工作空间 →
              </button>
            </div>
          )}
          {onboardStep > 0 && onboardStep < 3 && (
            <div className="onboard-navigation">
              <button
                className="ghost"
                disabled={generating}
                onClick={() => setOnboardStep(onboardStep - 1)}
              >
                ← 返回
              </button>
              <button
                className="primary"
                disabled={
                  generating || (onboardStep === 1 && !keys[settings.provider])
                }
                onClick={() => setOnboardStep(onboardStep + 1)}
              >
                {onboardStep === 2 && !knowledge ? "稍后准备知识库" : "继续 →"}
              </button>
            </div>
          )}
        </main>
      </div>
    );
  return (
    <>
      <aside className="sidebar">
        <a className="brand" href="#home" onClick={() => nav("home")}>
          <span className="brand-icon">
            <i />
            <i />
            <i />
            <i />
          </span>
          livecap
        </a>
        <div className="workspace">
          <span className="avatar">L</span>
          <span>
            我的工作空间<small>本地空间</small>
          </span>
        </div>
        <div className="nav-label">工作空间</div>
        <nav>
          {[
            "home",
            "records",
            "knowledge",
            ...(active ? ["session"] : []),
            "usage",
          ].map((p) => (
            <a
              key={p}
              href={"#" + p}
              className={page === p ? "active" : ""}
              onClick={() => nav(p)}
            >
              <span className="nav-icon">{navIcons[p]}</span>
              {names[p]}
              {p === "records" && (
                <small className="nav-count">{records.length}</small>
              )}
              {p === "knowledge" && generating && (
                <small className="nav-count">整理中</small>
              )}
            </a>
          ))}
        </nav>
        <div className="sidebar-bottom">
          <nav>
            <a
              href="#settings"
              className={page === "settings" ? "active" : ""}
              onClick={() => nav("settings")}
            >
              <span className="nav-icon">
                <Settings2 />
              </span>
              设置
            </a>
          </nav>
        </div>
      </aside>
      <div className={page === "usage" ? "shell usage-shell" : "shell"}>
        <main
          className={
            record
              ? "native-record-main"
              : page === "usage"
                ? "native-usage-main"
                : ""
          }
        >
          {error && (
            <div role="alert" className="native-error">
              {error}
              <button onClick={() => setError("")}>×</button>
            </div>
          )}
          {!ready ? (
            <p>正在读取本地数据…</p>
          ) : record ? (
            <RecordView
              key={record.id}
              summaryStatus={summaryStatus[record.id]}
              record={record}
              onBack={() => setRecord(null)}
              onChange={(r) => {
                setRecord(r);
                void refresh();
              }}
              fail={fail}
            />
          ) : (
            <>
              {page === "home" && (
                <>
                  <h1>听懂，然后留下收获。</h1>
                  <p className="subtitle">
                    播放会议或视频，实时查看字幕。按场景选择字幕翻译或会议翻译。
                  </p>
                  <div className="native-mode-picker">
                    <button
                      disabled={active}
                      className={mode === "subtitle" ? "selected" : ""}
                      onClick={() => setMode("subtitle")}
                    >
                      <span className="mode-icon">
                        <CaptionsIcon />
                      </span>
                      <h2>字幕翻译</h2>
                      <p>看电影、视频和直播</p>
                      <small>实时双语字幕 · 不保存音频和文字</small>
                    </button>
                    <button
                      disabled={active}
                      className={mode === "meeting" ? "selected" : ""}
                      onClick={() => setMode("meeting")}
                    >
                      <span className="mode-icon">
                        <UsersRound />
                      </span>
                      <h2>会议翻译</h2>
                      <p>会议、访谈和讨论</p>
                      <small>实时字幕 · 自动录音 · 纪要与回顾</small>
                    </button>
                  </div>
                  <section className="native-start">
                    <div>
                      <h2>
                        {mode === "subtitle" ? "字幕翻译" : "会议翻译"} ·
                        系统音频
                      </h2>
                      <p>
                        自动识别 → {languageName(settings.targetLanguage)} ·{" "}
                        {settings.provider === "openai" ? "OpenAI" : "Soniox"}
                      </p>
                    </div>
                    <button
                      className="primary"
                      disabled={!desktop || busy || !ready || generating}
                      onClick={() =>
                        active
                          ? nav("session")
                          : keys[settings.provider]
                            ? setConsent(true)
                            : nav("settings")
                      }
                    >
                      {active ? (
                        "查看当前会话"
                      ) : keys[settings.provider] ? (
                        <>
                          <Play />
                          {mode === "subtitle" ? "开始字幕" : "开始会议"}
                        </>
                      ) : (
                        "配置 API Key"
                      )}
                    </button>
                  </section>
                  {mode === "meeting" && (
                    <>
                      <div className="heading-row">
                        <h2>最近会议</h2>
                        <button
                          className="ghost"
                          onClick={() => nav("records")}
                        >
                          查看全部 →
                        </button>
                      </div>
                      <RecordList
                        records={records.slice(0, 5)}
                        open={openRecord}
                      />
                    </>
                  )}
                  {!knowledge && (
                    <section className="native-start native-knowledge-start">
                      <div>
                        <h3>准备你的领域知识</h3>
                        <p>让专业术语和背景资料参与翻译。</p>
                      </div>
                      <button onClick={() => nav("knowledge")}>
                        建立知识库 →
                      </button>
                    </section>
                  )}
                </>
              )}
              {page === "session" && (
                <>
                  <div className="heading-row">
                    <h1>当前会话</h1>
                    <div className="native-actions">
                      {mode === "meeting" &&
                        ["listening", "paused"].includes(status.state) && (
                          <button
                            disabled={busy}
                            onClick={() => void pauseMeeting()}
                          >
                            {status.state === "paused" ? <Play /> : <Pause />}
                            {status.state === "paused"
                              ? "继续会议"
                              : "暂停会议"}
                          </button>
                        )}
                      <button onClick={() => void overlay().catch(fail)}>
                        打开字幕窗
                      </button>
                      <button
                        className="primary"
                        disabled={
                          !active || busy || status.state === "stopping"
                        }
                        onClick={() => void stop()}
                      >
                        {status.state === "stopping" ? "正在保存…" : "结束会话"}
                      </button>
                    </div>
                  </div>
                  <div className="native-session-meta">
                    <span>
                      ●{" "}
                      {
                        (
                          {
                            connecting: "连接中",
                            listening:
                              mode === "meeting"
                                ? "实时转写 · 录音中"
                                : "实时字幕 · 不保存",
                            paused: "会议已暂停 · 不录音",
                            stopping: "等待最后结果",
                            idle: mode === "meeting" ? "已保存" : "已结束",
                            error: "会话中断",
                          } as Record<string, string>
                        )[status.state]
                      }
                    </span>
                    <span>预计费用 {money(usage?.estimated_usd)}</span>
                  </div>
                  {status.state === "paused" && (
                    <p role="status">
                      已暂停：新音频不会保存或发送，点击“继续会议”后接着录制到同一条记录。
                      {settings.provider === "soniox" &&
                        "Soniox 在线连接仍按连接时长计费。"}
                    </p>
                  )}
                  <div className="native-caption-preview">
                    <Captions snapshot={{ captions, status, settings, mode }} />
                  </div>
                  {mode === "meeting" && (
                    <div className="native-live-transcript">
                      {captions.confirmed.map((s) => (
                        <p
                          key={s.id}
                          className={
                            s.lane === "translation" ? "translated" : ""
                          }
                        >
                          {s.text}
                        </p>
                      ))}
                    </div>
                  )}
                </>
              )}
              {page === "records" && (
                <>
                  <div className="heading-row">
                    <h1>记录</h1>
                    <input
                      placeholder="搜索记录"
                      value={query}
                      onChange={(e) => setQuery(e.target.value)}
                    />
                  </div>
                  <RecordList
                    records={records.filter((r) =>
                      r.title.toLowerCase().includes(query.toLowerCase()),
                    )}
                    open={openRecord}
                  />
                </>
              )}

              {page === "usage" && (
                <UsageView
                  records={[
                    ...usageEntries.map((u) => ({
                      id: u.id,
                      title:
                        records.find((r) => r.id === u.recordId)?.title ||
                        (u.mode === "subtitle" ? "字幕翻译" : "会议翻译"),
                      createdAt: u.createdAt,
                      durationMs: u.usage.audio_ms,
                      usage: u.usage,
                    })),
                    ...knowledgeUsage.map((k) => ({
                      id: "knowledge-" + k.id,
                      title: "知识准备 · " + k.domain,
                      createdAt: k.createdAt,
                      durationMs: 0,
                      knowledge: k.usage,
                    })),
                  ].sort(
                    (a, b) => Date.parse(b.createdAt) - Date.parse(a.createdAt),
                  )}
                />
              )}
              {page === "settings" && (
                <>
                  <h1>设置</h1>
                  <ProductSettings
                    settings={settings}
                    setSettings={setSettings}
                    keys={keys}
                    onKeys={setKeys}
                    active={active}
                    preview={() => void overlay().catch(fail)}
                    fail={fail}
                  />
                </>
              )}
            </>
          )}
          {ready && (
            <div hidden={page !== "knowledge" || Boolean(record)}>
              {learningNotice && (
                <p className="subtitle" role="status">
                  {learningNotice}
                </p>
              )}
              <ProductKnowledge
                cache={knowledge}
                onCache={setKnowledge}
                hasKey={Boolean(keys.openai)}
                active={active}
                fail={fail}
                onGenerating={setGenerating}
                openSettings={() => nav("settings")}
                learned={learned}
                settings={settings}
                onSettings={setSettings}
              />
            </div>
          )}
        </main>
      </div>
      {consent && (
        <Dialog
          title={mode === "meeting" ? "开始会议翻译" : "开始字幕翻译"}
          close={() => setConsent(false)}
        >
          <p>
            当前播放设备的声音会发送至{" "}
            {settings.provider === "openai"
              ? "OpenAI，转录文字由 Luna 翻译"
              : "Soniox"}
            。
            {mode === "meeting"
              ? "录音和字幕将自动保存到本机。"
              : "本次不保存音频和文字。"}
          </p>
          <p>
            {mode === "meeting" ? "请确保会议参与者知悉录音。" : ""}
            {mode === "meeting" &&
            (settings.provider === "openai" || keys.openai)
              ? ` 归档后，原文将发送给 OpenAI 生成${settings.autoGenerateMeetingNotes ? "标题、纪要和关键时间线" : "标题和关键时间线"}，会产生模型费用。`
              : ""}
            {mode === "meeting" &&
            settings.autoLearnMeetingTerms &&
            settings.provider === "openai" &&
            settings.targetLanguage === "zh"
              ? " 会后会自动查证并补充术语。"
              : ""}
            采集包含同设备上的通知与其他声音。
          </p>
          {mode === "meeting" && (
            <label className="edit-label">
              会议主题（可选）
              <input
                maxLength={160}
                value={meetingTopic}
                onChange={(e) => setMeetingTopic(e.target.value)}
                placeholder="例如 Clang 前端、AST 与语义分析"
              />
              <small>用来优先选取本场会议的专业热词。</small>
            </label>
          )}
          <div className="dialog-actions">
            <button onClick={() => setConsent(false)}>取消</button>
            <button className="primary" onClick={() => void start()}>
              {mode === "meeting" ? "开始会议" : "开始字幕"}
            </button>
          </div>
        </Dialog>
      )}
    </>
  );
}
function RecordList({
  records,
  open,
}: {
  records: RecordSummary[];
  open: (id: number) => void;
}) {
  return (
    <div className="document-list">
      {records.length ? (
        records.map((r) => (
          <button
            className="document-card"
            key={r.id}
            onClick={() => open(r.id)}
          >
            <div className="document-icon">
              <FileText />
            </div>
            <div className="document-info">
              <div className="document-title">
                <strong>{r.title}</strong>
                <span className="document-status">
                  {r.status === "partial" ? "部分记录" : "已保存"}
                </span>
              </div>
              <small>
                {date(r.createdAt)} · {time(r.durationMs)} ·{" "}
                {r.audioAvailable ? "含录音" : "文字记录"}
              </small>
            </div>
            <span className="document-arrow">
              <ArrowUpRight />
            </span>
          </button>
        ))
      ) : (
        <div className="records-empty">
          <FileText />
          <h3>还没有会议记录</h3>
          <p>结束一次会议翻译后，录音、原文、译文与纪要会显示在这里。</p>
        </div>
      )}
    </div>
  );
}
function replay(record: RecordDetail) {
  let c = createCaptionState(Number.MAX_SAFE_INTEGER);
  if (record.transcriptEvents[0])
    c.sessionId = record.transcriptEvents[0].session_id;
  for (const event of record.transcriptEvents) c = applyCaptionFrame(c, event);
  return c;
}
function RecordView({
  record: r,
  onBack,
  onChange,
  fail,
  summaryStatus,
}: {
  record: RecordDetail;
  summaryStatus?: SummaryStatus;
  onBack: () => void;
  onChange: (r: RecordDetail) => void;
  fail: (e: unknown) => void;
}) {
  const [split, setSplit] = useState(42),
    [compare, setCompare] = useState(true),
    [follow, setFollow] = useState(true),
    [readView, setReadView] = useState("visual"),
    [segmentEdit, setSegmentEdit] = useState<{
      id: string;
      text: string;
      speaker: string;
    } | null>(null);
  const [tab, setTab] = useState("minutes"),
    [language, setLanguage] = useState("original"),
    [search, setSearch] = useState(""),
    [notes, setNotes] = useState(r.notes || ""),
    [editing, setEditing] = useState(false),
    [title, setTitle] = useState(r.title),
    [audioUrl, setAudioUrl] = useState(""),
    [loading, setLoading] = useState(false),
    [playing, setPlaying] = useState(false),
    [position, setPosition] = useState(0),
    [rate, setRate] = useState(1),
    [question, setQuestion] = useState(""),
    [answer, setAnswer] = useState(""),
    [working, setWorking] = useState(false),
    [editingTranscript, setEditingTranscript] = useState(false),
    [transcriptDraft, setTranscriptDraft] = useState("");
  const [askOpen, setAskOpen] = useState(false),
    [asking, setAsking] = useState(false),
    [askError, setAskError] = useState("");
  const titleFocused = useRef(false);
  const titleDraftChanged = useRef(false);
  useEffect(() => {
    if (!titleFocused.current) setTitle(r.title);
  }, [r.title]);
  const summarizing = working || summaryStatus?.running;
  async function generateNotes() {
    setWorking(true);
    try {
      onChange(
        await api<RecordDetail>("generate_record_notes", {
          id: r.id,
          consent: true,
        }),
      );
    } catch (reason) {
      fail(reason);
    } finally {
      setWorking(false);
    }
  }
  async function askQuestion() {
    if (asking || !question.trim()) return;
    setAsking(true);
    setAskError("");
    try {
      setAnswer(
        (
          await api<{ answer: string }>("ask_record", {
            id: r.id,
            question,
            consent: true,
          })
        ).answer,
      );
    } catch (reason) {
      setAskError(reason instanceof Error ? reason.message : String(reason));
    } finally {
      setAsking(false);
    }
  }
  const audio = useRef<HTMLAudioElement>(null);
  const pendingSeek = useRef<{ ms: number; play: boolean } | null>(null);
  const [citationJump, setCitationJump] = useState(0);
  const lastCitationJump = useRef(0);
  const transcriptPane = useRef<HTMLDivElement>(null);
  const c = useMemo(() => {
    const state = replay(r);
    state.confirmed = state.confirmed.map((segment) =>
      r.segmentEdits?.[segment.id]
        ? { ...segment, ...r.segmentEdits[segment.id] }
        : segment,
    );
    return state;
  }, [r]);
  const originals = useMemo(
    () => c.confirmed.filter((s) => s.lane === "original"),
    [c],
  );
  const translations = c.confirmed.filter((s) => s.lane === "translation");
  useEffect(() => {
    let disposed = false,
      url = "";
    if (!r.audioAvailable) return;
    setLoading(true);
    void (async () => {
      const chunks: Uint8Array[] = [];
      let offset = 0,
        total = 1;
      while (offset < total) {
        const result = await api<{ data: string; totalBytes: number }>(
          "read_record_audio",
          { id: r.id, offset, length: 1024 * 1024 },
        );
        if (disposed) return;
        const raw = atob(result.data);
        const bytes = Uint8Array.from(raw, (x) => x.charCodeAt(0));
        if (!bytes.length) throw Error("录音读取不完整");
        chunks.push(bytes);
        offset += bytes.length;
        total = result.totalBytes;
      }
      url = URL.createObjectURL(
        new Blob(chunks as BlobPart[], { type: "audio/wav" }),
      );
      if (!disposed) setAudioUrl(url);
    })()
      .catch(fail)
      .finally(() => {
        if (!disposed) setLoading(false);
      });
    return () => {
      disposed = true;
      if (url) URL.revokeObjectURL(url);
    };
  }, [r.id]);
  const save = async (patch: Record<string, unknown>) => {
    try {
      onChange(await api<RecordDetail>("update_record", { id: r.id, patch }));
      setEditing(false);
      return true;
    } catch (e) {
      fail(e);
      return false;
    }
  };
  function seek(ms: number, play = false) {
    if (!r.audioAvailable) return;
    const player = audio.current;
    if (player && player.readyState > 0) {
      player.currentTime = ms / 1000;
      setPosition(ms);
      if (play) void player.play().catch(fail);
    } else {
      pendingSeek.current = { ms, play };
      setCompare(true);
    }
  }
  function listenFromCitation(ms: number) {
    setLanguage((value) => (value === "translation" ? "original" : value));
    setSearch("");
    setCitationJump((value) => value + 1);
    seek(ms, true);
  }
  const transcript = r.transcriptText || r.legacyContent || exportCaptions(c);
  const sentenceGroups = useMemo(
    () =>
      originals.map((segment) =>
        timedSentences(segment.text, segment.startMs, segment.endMs),
      ),
    [originals],
  );
  const currentIndex = originals.reduce(
    (last, segment, index) =>
      segment.startMs != null &&
      segment.endMs != null &&
      segment.startMs <= position &&
      position < segment.endMs
        ? index
        : last,
    -1,
  );
  const currentSentence =
    sentenceGroups[currentIndex]?.findIndex(
      (sentence) =>
        sentence.startMs != null &&
        sentence.endMs != null &&
        sentence.startMs <= position &&
        position < sentence.endMs,
    ) ?? -1;
  useEffect(() => {
    const forced = citationJump !== lastCitationJump.current;
    if (!compare || currentIndex < 0 || (!forced && (!playing || !follow)))
      return;
    if (forced && pendingSeek.current) return;
    const pane = transcriptPane.current;
    const sentenceNode =
      currentSentence >= 0
        ? pane?.querySelector(
            `[data-record-sentence="${currentIndex}-${currentSentence}"]`,
          )
        : null;
    const node =
      sentenceNode ?? pane?.querySelector(`#record-segment-${currentIndex}`);
    if (!pane || !node) return;
    lastCitationJump.current = citationJump;
    const bounds = pane.getBoundingClientRect();
    const target = node.getBoundingClientRect();
    if (forced || target.top < bounds.top || target.bottom > bounds.bottom) {
      pane.scrollTo({
        top: pane.scrollTop + target.top - bounds.top - pane.clientHeight / 3,
        behavior: forced ? "auto" : "smooth",
      });
    }
  }, [
    citationJump,
    compare,
    currentIndex,
    currentSentence,
    playing,
    follow,
    search,
    language,
  ]);
  function download() {
    const url = URL.createObjectURL(
      new Blob([r.title + "\n\n" + r.notes + "\n\n" + transcript], {
        type: "text/plain;charset=utf-8",
      }),
    );
    const a = document.createElement("a");
    a.href = url;
    a.download = r.title + ".txt";
    a.click();
    setTimeout(() => URL.revokeObjectURL(url), 1000);
  }
  return (
    <div className="native-record">
      <header className="native-record-heading">
        <button
          className="record-back"
          aria-label="返回记录列表"
          onClick={onBack}
        >
          <ArrowLeft />
        </button>
        <div>
          <input
            aria-label="记录标题"
            value={title}
            onChange={(e) => {
              titleDraftChanged.current = true;
              setTitle(e.target.value);
            }}
            onFocus={() => {
              titleFocused.current = true;
              titleDraftChanged.current = false;
            }}
            onBlur={() => {
              titleFocused.current = false;
              if (
                titleDraftChanged.current &&
                title.trim() &&
                title !== r.title
              )
                void save({ title });
              else setTitle(r.title);
            }}
          />
          <small>
            {date(r.createdAt)} · {time(r.durationMs)} ·{" "}
            {r.provider || "本地记录"}
            {r.targetLanguage ? " · " + languageName(r.targetLanguage) : ""}
          </small>
        </div>
        <button className="record-export" onClick={download}>
          <Download /> 导出
        </button>
        <button aria-label="切换原文对照" onClick={() => setCompare(!compare)}>
          {compare ? "收起原文" : "显示原文"}
        </button>
      </header>
      {r.error && <div className="native-error">部分内容未完成：{r.error}</div>}
      <div
        className={"native-record-columns " + (!compare ? "document-only" : "")}
        style={{
          gridTemplateColumns: compare
            ? `${split}fr 8px ${100 - split}fr`
            : undefined,
        }}
      >
        <section className="native-document">
          <div className="native-tabs">
            {[
              ["overview", "概览"],
              ["minutes", "纪要"],
              ["timeline", "时间线"],
            ].map(([id, label]) => (
              <button
                key={id}
                className={tab === id ? "active" : ""}
                onClick={() => setTab(id)}
              >
                {label}
              </button>
            ))}
            <button
              className="ghost record-edit"
              onClick={() => {
                setNotes(r.notes || "");
                setEditing(true);
              }}
            >
              <Pencil /> 编辑
            </button>
          </div>
          <div className="record-reading-toolbar">
            <select
              aria-label="纪要阅读视图"
              value={readView}
              onChange={(e) => setReadView(e.target.value)}
            >
              <option value="visual">图文阅读</option>
              <option value="plain">纯文本</option>
            </select>
            <button
              className="ghost small record-copy"
              onClick={() =>
                void navigator.clipboard
                  .writeText(r.notes || transcript)
                  .catch(fail)
              }
            >
              <Copy /> 复制
            </button>
          </div>
          <div className="native-document-scroll">
            {tab === "timeline" ? (
              r.timeline?.length ? (
                <ol className="record-key-timeline" aria-label="关键时间线">
                  {r.timeline.map((point, index) => (
                    <li key={index}>
                      <div className="timeline-node">
                        {point.startMs != null ? (
                          r.audioAvailable ? (
                            <button
                              className="note-citation"
                              onClick={() => seek(point.startMs!)}
                            >
                              {time(point.startMs)}
                            </button>
                          ) : (
                            <span>{time(point.startMs)}</span>
                          )
                        ) : (
                          <span>{index + 1}</span>
                        )}
                        <h3>{point.title}</h3>
                      </div>
                      <p>{point.summary}</p>
                      {!!point.details.length && (
                        <ul>
                          {point.details.map((detail, i) => (
                            <li key={i}>{detail}</li>
                          ))}
                        </ul>
                      )}
                    </li>
                  ))}
                </ol>
              ) : (
                <div className="native-empty">
                  <h2>关键时间线</h2>
                  <p>从会话中提炼几个可以回听的重点。</p>
                  <button
                    className="primary"
                    disabled={summarizing}
                    onClick={() => void generateNotes()}
                  >
                    {summarizing ? "正在整理…" : "生成纪要与时间线"}
                  </button>
                  {summaryStatus?.error && (
                    <small role="status">{summaryStatus.error}</small>
                  )}
                </div>
              )
            ) : editing ? (
              <>
                <textarea
                  aria-label="纪要内容"
                  rows={20}
                  value={notes}
                  onChange={(e) => setNotes(e.target.value)}
                />
                <button
                  className="primary"
                  onClick={() => void save({ notes })}
                >
                  保存纪要
                </button>
              </>
            ) : r.notes || r.timeline?.length ? (
              <>
                {readView === "visual" &&
                  (r.diagram ? (
                    <MeetingDiagram diagram={r.diagram} />
                  ) : (
                    <div className="record-diagram-empty">
                      <p>重新整理可生成内容关系图。</p>
                      <button
                        className="small"
                        disabled={summarizing}
                        onClick={() => void generateNotes()}
                      >
                        {summarizing ? "正在整理…" : "生成图文纪要"}
                      </button>
                    </div>
                  ))}
                {r.notes && (
                  <NotesDocument
                    plain={readView === "plain"}
                    text={
                      tab === "overview" && readView !== "plain"
                        ? r.notes.split("\n").slice(0, 18).join("\n")
                        : r.notes
                    }
                    seek={r.audioAvailable ? listenFromCitation : undefined}
                    sourceTimes={r.noteSources}
                  />
                )}
              </>
            ) : (
              <div className="native-empty">
                <span>✧</span>
                <h2>整理这次会话</h2>
                <p>从原文提炼主题、结论与后续事项。</p>
                <button
                  className="primary"
                  disabled={summarizing || !transcript.trim()}
                  onClick={() => void generateNotes()}
                >
                  {summarizing ? "正在整理…" : "生成纪要与时间线"}
                </button>
                {summaryStatus?.error && (
                  <small role="status">{summaryStatus.error}</small>
                )}
                <small>会将本条记录的转录文字发送至 OpenAI。</small>
              </div>
            )}
          </div>
        </section>
        {compare && (
          <>
            <div
              className="record-splitter"
              role="separator"
              tabIndex={0}
              aria-label="调整纪要与原文宽度"
              aria-orientation="vertical"
              aria-valuenow={split}
              aria-valuemin={30}
              aria-valuemax={65}
              onKeyDown={(e) => {
                if (e.key === "ArrowLeft") setSplit((v) => Math.max(30, v - 2));
                if (e.key === "ArrowRight")
                  setSplit((v) => Math.min(65, v + 2));
              }}
              onPointerDown={(e) => {
                const node = e.currentTarget;
                node.setPointerCapture(e.pointerId);
              }}
              onPointerMove={(e) => {
                if (!e.currentTarget.hasPointerCapture(e.pointerId)) return;
                const box =
                  e.currentTarget.parentElement!.getBoundingClientRect();
                setSplit(
                  Math.max(
                    30,
                    Math.min(65, ((e.clientX - box.left) / box.width) * 100),
                  ),
                );
              }}
              onPointerUp={(e) =>
                e.currentTarget.releasePointerCapture(e.pointerId)
              }
            >
              <i />
            </div>
            <section className="native-source">
              <div className="audio-player">
                <div className="audio-heading">
                  <b>
                    <AudioLines /> 会话录音
                  </b>
                  <small>
                    {loading
                      ? "正在加载…"
                      : r.audioAvailable
                        ? "本地录音"
                        : "此记录没有音频"}
                  </small>
                </div>
                <audio
                  ref={audio}
                  src={audioUrl || undefined}
                  onLoadedMetadata={() => {
                    const request = pendingSeek.current;
                    if (request) {
                      pendingSeek.current = null;
                      seek(request.ms, request.play);
                    }
                  }}
                  onTimeUpdate={() =>
                    setPosition((audio.current?.currentTime || 0) * 1000)
                  }
                  onPlay={() => setPlaying(true)}
                  onPause={() => setPlaying(false)}
                  onEnded={() => setPlaying(false)}
                  onError={() => fail("无法播放这条录音。")}
                />
                <input
                  aria-label="播放进度"
                  type="range"
                  min="0"
                  max={Math.max(r.durationMs, 1)}
                  value={position}
                  onChange={(e) => seek(Number(e.target.value))}
                  disabled={!audioUrl}
                />
                <div className="audio-controls">
                  <button
                    aria-label={playing ? "暂停录音" : "播放录音"}
                    disabled={!audioUrl}
                    onClick={() => {
                      if (audio.current) {
                        if (playing) audio.current.pause();
                        else void audio.current.play().catch(fail);
                      }
                    }}
                  >
                    {playing ? <Pause /> : <Play />}
                  </button>
                  <span>
                    {time(position)} / {time(r.durationMs)}
                  </span>
                  <span className="audio-spacer" />
                  <button
                    aria-pressed={follow}
                    onClick={() => setFollow(!follow)}
                  >
                    {follow ? "跟随播放" : "自由阅读"}
                  </button>
                  <small
                    className="sentence-timing-label"
                    title="录音按段落保存时间，句子位置根据段落时长估算。"
                  >
                    句子位置估算
                  </small>
                  <select
                    aria-label="播放速度"
                    value={rate}
                    onChange={(e) => {
                      const v = Number(e.target.value);
                      setRate(v);
                      if (audio.current) audio.current.playbackRate = v;
                    }}
                  >
                    {[0.75, 1, 1.25, 1.5, 2].map((n) => (
                      <option key={n} value={n}>
                        {n}×
                      </option>
                    ))}
                  </select>
                  {audioUrl && (
                    <a href={audioUrl} download={r.title + ".wav"}>
                      ↓ 下载
                    </a>
                  )}
                </div>
              </div>
              <div className="native-source-heading">
                <h3>原文与翻译</h3>
                <button
                  onClick={() => {
                    setTranscriptDraft(transcript);
                    setEditingTranscript(true);
                  }}
                >
                  校对
                </button>
                <select
                  aria-label="显示语言"
                  value={language}
                  onChange={(e) => setLanguage(e.target.value)}
                >
                  <option value="original">原文</option>
                  <option value="both">双语</option>
                  <option value="translation">译文</option>
                </select>
              </div>
              <input
                placeholder="搜索原文"
                value={search}
                onChange={(e) => setSearch(e.target.value)}
              />
              <div className="native-transcript-scroll" ref={transcriptPane}>
                {editingTranscript ? (
                  <>
                    <textarea
                      aria-label="校对文本"
                      rows={18}
                      value={transcriptDraft}
                      onChange={(e) => setTranscriptDraft(e.target.value)}
                    />
                    <button
                      onClick={() => {
                        void save({ transcriptText: transcriptDraft }).then(
                          (saved) => {
                            if (saved) setEditingTranscript(false);
                          },
                        );
                      }}
                    >
                      保存校对
                    </button>
                    <button onClick={() => setEditingTranscript(false)}>
                      取消
                    </button>
                  </>
                ) : (r.transcriptText &&
                    !Object.keys(r.segmentEdits || {}).length) ||
                  r.legacyContent ? (
                  <pre>{transcript}</pre>
                ) : (
                  originals.map((s, index) => {
                    if (!s.text.toLowerCase().includes(search.toLowerCase()))
                      return null;
                    const translation = translations.find(
                      (t) =>
                        t.externalTranslation?.sourceSegmentId === s.id ||
                        Boolean(
                          s.providerItemId &&
                          t.providerItemId === s.providerItemId,
                        ),
                    );
                    return (
                      <article id={"record-segment-" + index} key={s.id}>
                        <header>
                          <span>
                            ◉ {s.speaker ? "说话人 " + s.speaker : "发言者"}
                          </span>
                          <button onClick={() => seek(s.startMs || 0)}>
                            {time(s.startMs || 0)}
                          </button>
                          <button
                            className="paragraph-edit"
                            aria-label={"校对段落 " + time(s.startMs || 0)}
                            onClick={() =>
                              setSegmentEdit({
                                id: s.id,
                                text: s.text,
                                speaker: s.speaker || "",
                              })
                            }
                          >
                            <Pencil />
                          </button>
                        </header>
                        {language !== "translation" && (
                          <p>
                            {sentenceGroups[index].map(
                              (sentence, sentenceIndex) => (
                                <span
                                  key={sentenceIndex}
                                  data-record-sentence={`${index}-${sentenceIndex}`}
                                  className={
                                    index === currentIndex &&
                                    sentenceIndex === currentSentence
                                      ? "record-sentence-active"
                                      : undefined
                                  }
                                >
                                  {sentence.text}
                                </span>
                              ),
                            )}
                          </p>
                        )}
                        {language !== "original" && translation && (
                          <p className="translated">
                            {translation.text}
                            <button
                              className="paragraph-edit"
                              aria-label="校对译文"
                              onClick={() =>
                                setSegmentEdit({
                                  id: translation.id,
                                  text: translation.text,
                                  speaker: translation.speaker || "",
                                })
                              }
                            >
                              <Pencil />
                            </button>
                          </p>
                        )}
                      </article>
                    );
                  })
                )}
                {r.provider === "soniox" && language !== "original" && (
                  <section>
                    <h3>译文</h3>
                    {translations.map((t) => (
                      <p key={t.id}>{t.text}</p>
                    ))}
                  </section>
                )}
              </div>
            </section>
          </>
        )}
      </div>
      <div className="record-ask-float">
        {askOpen && (
          <section className="record-ask-panel" aria-label="问这条记录">
            <header>
              <h3>
                <Sparkles size={16} /> 问这条记录
              </h3>
              <button
                className="ghost"
                aria-label="收起提问"
                onClick={() => setAskOpen(false)}
              >
                <X size={16} />
              </button>
            </header>
            <div className="record-prompts">
              {["提炼核心结论", "列出待办事项", "有哪些未解决的问题？"].map(
                (prompt) => (
                  <button key={prompt} onClick={() => setQuestion(prompt)}>
                    {prompt}
                  </button>
                ),
              )}
            </div>
            {answer && (
              <div className="record-answer">
                <NotesDocument text={answer} seek={seek} />
              </div>
            )}
            {askError && <p role="alert">{askError}</p>}
            <form
              onSubmit={(event) => {
                event.preventDefault();
                void askQuestion();
              }}
            >
              <input
                aria-label="问题"
                placeholder="提问时将原文发送至 OpenAI"
                value={question}
                onChange={(event) => setQuestion(event.target.value)}
              />
              <button
                className="primary"
                aria-label="发送问题"
                disabled={asking || !question.trim()}
              >
                {asking ? "查询中…" : <ArrowUpRight />}
              </button>
            </form>
          </section>
        )}
        <button
          className="primary record-ask-trigger"
          aria-expanded={askOpen}
          onClick={() => setAskOpen(!askOpen)}
        >
          <MessageCircle size={17} /> 问这条记录
        </button>
      </div>
      {segmentEdit && (
        <Dialog title="校对段落" close={() => setSegmentEdit(null)}>
          <label className="edit-label">
            说话人
            <input
              aria-label="说话人"
              value={segmentEdit.speaker}
              onChange={(e) =>
                setSegmentEdit({ ...segmentEdit, speaker: e.target.value })
              }
            />
          </label>
          <textarea
            aria-label="段落内容"
            rows={8}
            value={segmentEdit.text}
            onChange={(e) =>
              setSegmentEdit({ ...segmentEdit, text: e.target.value })
            }
          />
          <div className="dialog-actions">
            <button onClick={() => setSegmentEdit(null)}>取消</button>
            <button
              className="primary"
              onClick={() => {
                const edits = {
                  ...r.segmentEdits,
                  [segmentEdit.id]: {
                    text: segmentEdit.text,
                    speaker: segmentEdit.speaker,
                  },
                };
                const source = originals
                  .map(
                    (x) =>
                      `[${time(x.startMs || 0)}] ${edits[x.id]?.speaker || x.speaker || "发言者"}：${edits[x.id]?.text ?? x.text}`,
                  )
                  .join("\n");
                void save({ segmentEdits: edits, transcriptText: source }).then(
                  (saved) => {
                    if (saved) setSegmentEdit(null);
                  },
                );
              }}
            >
              保存修改
            </button>
          </div>
        </Dialog>
      )}
    </div>
  );
}
type UsageRow = {
  id: string;
  title: string;
  createdAt: string;
  durationMs: number;
  usage?: Usage;
  knowledge?: KnowledgeUsage;
};
const usageCost = (row: UsageRow) =>
  row.knowledge ? row.knowledge.estimatedUsd : row.usage?.estimated_usd;
const costColors = ["#738c61", "#9caf88", "#b4a3c7", "#ce8b6f"];
function UsageTokens({ value }: { value: number | null | undefined }) {
  if (value == null) return <>—</>;
  const amount = value / 1_000_000;
  const display =
    value > 0 && value < 100
      ? "<0.0001"
      : amount.toLocaleString("zh-CN", { maximumFractionDigits: 4 });
  return (
    <span title={`${value.toLocaleString("zh-CN")} tokens`}>{display} M</span>
  );
}
function UsageView({ records }: { records: UsageRow[] }) {
  const [days, setDays] = useState(7),
    [metric, setMetric] = useState("cost"),
    [selected, setSelected] = useState<number | null>(null);
  const now = new Date();
  now.setHours(0, 0, 0, 0);
  const start = now.getTime() - (days - 1) * 86400000;
  const events = records.filter((r) => Date.parse(r.createdAt) >= start);
  const sum = (
    field: "transcription_usd" | "translation_usd" | "analysis_usd",
  ) => events.reduce((n, r) => n + (r.usage?.[field] || 0), 0);
  const total = events.reduce((n, r) => n + (usageCost(r) ?? 0), 0),
    minutes = events.reduce((n, r) => n + r.durationMs / 60000, 0),
    known = events.filter((r) => usageCost(r) != null).length;
  const daily = Array.from({ length: days }, (_, i) => {
    const d = start + i * 86400000;
    const rs = events.filter(
      (r) =>
        Date.parse(r.createdAt) >= d && Date.parse(r.createdAt) < d + 86400000,
    );
    return {
      day: d,
      records: rs,
      cost: rs.reduce((n, r) => n + (usageCost(r) ?? 0), 0),
      minutes: rs.reduce((n, r) => n + r.durationMs / 60000, 0),
    };
  });
  const max = Math.max(
    0.01,
    ...daily.map((d) => (metric === "cost" ? d.cost : d.minutes)),
  );
  const knowledgeRows = events.filter((r) => r.knowledge);
  const knownKnowledge = knowledgeRows.some((r) => usageCost(r) != null);
  const knownSession = events.some((r) => r.usage?.estimated_usd != null);
  const knowledgeCost = knowledgeRows.reduce(
    (n, r) => n + (r.knowledge?.estimatedUsd ?? 0),
    0,
  );
  const parts = [
    sum("transcription_usd"),
    sum("translation_usd"),
    sum("analysis_usd"),
    knowledgeCost,
  ];
  let offset = 0;
  const stops = parts.map((part, i) => {
    const start = offset;
    offset += part;
    return `${costColors[i]} ${(start / total) * 100}% ${(offset / total) * 100}%`;
  });
  const rows =
    selected === null
      ? events
      : daily.find((d) => d.day === selected)?.records || [];
  return (
    <div className="usage-dashboard">
      <div className="usage-page-heading">
        <div>
          <h1>用量</h1>
          <p>字幕、会议与知识准备的用量和预计费用</p>
        </div>
        <div className="segmented">
          {[7, 30].map((n) => (
            <button
              key={n}
              className={days === n ? "active" : ""}
              onClick={() => {
                setDays(n);
                setSelected(null);
              }}
            >
              近 {n} 天
            </button>
          ))}
        </div>
      </div>
      <div className="usage-hero">
        <div className="usage-total">
          <span>已统计费用 USD</span>
          <strong>{known ? money(total) : "—"}</strong>
          <p>
            {known} / {events.length} 条记录有费用数据
          </p>
        </div>
        <div>
          <span>收音时长</span>
          <strong>
            {Math.floor(minutes / 60)}
            <small>小时</small> {Math.floor(minutes % 60)}
            <small>分</small>
          </strong>
        </div>
        <div>
          <span>使用次数</span>
          <strong>
            {events.length}
            <small>条</small>
          </strong>
        </div>
        <div>
          <span>知识准备费用</span>
          <strong>{money(knownKnowledge ? knowledgeCost : null)}</strong>
        </div>
      </div>
      <div className="usage-charts">
        <section className="trend-card">
          <header>
            <h3>使用趋势</h3>
            <div className="segmented">
              {[
                ["cost", "费用"],
                ["minutes", "时长"],
              ].map(([id, label]) => (
                <button
                  key={id}
                  className={metric === id ? "active" : ""}
                  onClick={() => setMetric(id)}
                >
                  {label}
                </button>
              ))}
            </div>
          </header>
          <div className="chart-unit">{metric === "cost" ? "USD" : "分钟"}</div>
          <div className="bar-chart">
            {daily.map((d, i) => (
              <button
                key={d.day}
                title={`${new Date(d.day).toLocaleDateString()} · ${money(d.cost)} · ${d.minutes.toFixed(1)} 分钟`}
                onClick={() => setSelected(selected === d.day ? null : d.day)}
                className={selected === d.day ? "selected" : ""}
              >
                <span className="bar-value">
                  {metric === "cost" ? money(d.cost) : d.minutes.toFixed(0)}
                </span>
                <span
                  className="chart-bar"
                  style={{
                    height:
                      ((metric === "cost" ? d.cost : d.minutes) / max) * 76 +
                      2 +
                      "%",
                  }}
                />
                <small>
                  {days === 7 || i % 5 === 0
                    ? new Date(d.day).toLocaleDateString("zh-CN", {
                        month: "numeric",
                        day: "numeric",
                      })
                    : ""}
                </small>
              </button>
            ))}
          </div>
        </section>
        <section className="breakdown-card">
          <h3>费用构成</h3>
          <div
            className="cost-donut"
            style={{
              background: total
                ? `conic-gradient(${stops.join(", ")})`
                : "#e9ece5",
            }}
          >
            <span>
              <small>已统计费用</small>
              <strong>{known ? money(total) : "—"}</strong>
            </span>
          </div>
          <div className="cost-legend">
            {["语音转录", "字幕翻译", "纪要与问答", "知识准备"].map(
              (label, i) => (
                <div key={label}>
                  <i style={{ background: costColors[i] }} />
                  <span>{label}</span>
                  <b>
                    {(i === 3 ? knownKnowledge : knownSession)
                      ? money(parts[i])
                      : "—"}
                  </b>
                </div>
              ),
            )}
          </div>
        </section>
      </div>
      <section className="record-costs">
        <header>
          <h3 id="usage-cost-heading">费用明细</h3>
          {selected !== null && (
            <button onClick={() => setSelected(null)}>显示全部</button>
          )}
        </header>
        <div
          className="usage-cost-table"
          role="region"
          aria-labelledby="usage-cost-heading"
          tabIndex={0}
        >
          <table>
            <thead>
              <tr>
                {[
                  "记录",
                  "日期",
                  "时长",
                  "输入 Token",
                  "输出 Token",
                  "网页调用",
                  "费用",
                ].map((label) => (
                  <th scope="col" key={label}>
                    {label}
                  </th>
                ))}
              </tr>
            </thead>
            <tbody>
              {rows.map((row) => {
                const input =
                  row.knowledge?.inputTokens ?? row.usage?.input_tokens;
                const output =
                  row.knowledge?.outputTokens ?? row.usage?.output_tokens;
                return (
                  <tr key={row.id}>
                    <th scope="row">{row.title}</th>
                    <td>{date(row.createdAt)}</td>
                    <td>{row.knowledge ? "—" : time(row.durationMs)}</td>
                    <td>
                      <UsageTokens value={input} />
                    </td>
                    <td>
                      <UsageTokens value={output} />
                    </td>
                    <td>
                      {row.knowledge
                        ? `${row.knowledge.webSearchCalls} 次`
                        : "—"}
                    </td>
                    <td>
                      <strong
                        title={
                          row.usage
                            ? `语音转录 ${money(row.usage.transcription_usd)} · 字幕翻译 ${money(row.usage.translation_usd)} · 纪要与问答 ${money(row.usage.analysis_usd)}`
                            : undefined
                        }
                      >
                        {money(usageCost(row))}
                        {(row.knowledge?.incomplete ?? row.usage?.incomplete)
                          ? " *"
                          : ""}
                      </strong>
                    </td>
                  </tr>
                );
              })}
            </tbody>
          </table>
          {!rows.length && (
            <div className="records-empty">这段时间还没有用量记录</div>
          )}
        </div>
      </section>
      <p className="usage-footnote">
        费用根据本地统计的用量估算，缺失数据不计为零，*
        表示统计不完整；最终金额以服务商账单为准。知识准备统计已成功保存的任务。
      </p>
    </div>
  );
}

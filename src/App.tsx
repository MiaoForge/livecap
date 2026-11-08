import { useEffect, useLayoutEffect, useRef, useState } from "react";
import type { CSSProperties, FormEvent, ReactNode } from "react";
import { isTauri, invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import {
  ArrowDown,
  ArrowDownToLine,
  ArrowLeft,
  Captions,
  Check,
  ChevronUp,
  CircleAlert,
  FlaskConical,
  History,
  Minus,
  Pencil,
  Pin,
  Play,
  Save,
  Settings2,
  Square,
  Trash2,
  X,
} from "lucide-react";
import {
  applyCaptionFrame,
  applyCaptionTokens,
  clearCaptionDrafts,
  createCaptionState,
  demoTokenBatches,
  editCaptionSegment,
  exportCaptions,
  freezeCaptionSegment,
  renameSpeaker,
  speakerLabel,
} from "./captions";
import type {
  CaptionSegment,
  CaptionState,
  TranscriptFrame,
  TranscriptToken,
} from "./captions";
import { selectLiveCaptions } from "./captionPresentation";
import { useCaptionWindow } from "./windowLayout";
import { KnowledgeSettings } from "./KnowledgeSettings";
import "./App.css";

type Provider = "openai" | "soniox";

type Settings = {
  terms: string[];
  model: string;
  fontSize: number;
  alwaysOnTop: boolean;
  provider: Provider;
};

type SessionStatus = {
  state: "connecting" | "listening" | "stopping" | "idle" | "error";
  message?: string;
};

type SessionUsage = {
  session_id: string;
  provider: Provider;
  audio_ms: number;
  input_tokens: number;
  cached_input_tokens: number;
  output_tokens: number;
  estimated_usd: number | null;
  transcription_usd: number | null;
  translation_usd: number | null;
  incomplete: boolean;
  pending_translation_requests: number;
};

type LearnedTerm = { term: string; occurrences: number; active: boolean };
type SessionTerms = { session_id: string; candidates: LearnedTerm[] };

type Transcript = { id: number; createdAt: string; content: string };
type View = "captions" | "settings" | "history";
type Prompt = "live" | "demo" | "close" | null;
type Edit = {
  segment: CaptionSegment;
  text: string;
  speaker: string;
  name: string;
  newSpeaker: string;
};
const addSpeaker = "new-local-speaker";

const defaults: Settings = {
  terms: [],
  model: "stt-rt-v6",
  fontSize: 24,
  alwaysOnTop: true,
  provider: "openai",
};

const longPreview: TranscriptToken[] = [
  {
    text: "When we replay a recording of a long technical meeting, the captions can keep growing while one person finishes explaining a complex change. Some examples mention Clang, intermediate representation, or compiler optimization, and several sentences may share the same subtitle area. The reading window should follow the words as they arrive even when the text has already wrapped several times, so the end of this long caption stays visible.",
    is_final: true,
    translation_status: "original",
    source_segment_id: "preview-long",
    revision: 1,
  },
  {
    text: "重放一段很长的技术会议录音时，只要发言者仍在解释一项复杂的变更，字幕就会不断增加。有些例子会提到 Clang、中间表示或者编译器优化，几句话也可能共用同一片字幕区域。当内容已经换行好几次，阅读窗口也应该继续跟随新出现的文字。现在应该能看到长句的最后一句。",
    is_final: true,
    translation_status: "translation",
    source_segment_id: "preview-long",
    revision: 1,
    translation_source: "openai",
  },
];

const providerNames: Record<Provider, string> = {
  openai: "OpenAI",
  soniox: "Soniox",
};

const statusLabel: Record<SessionStatus["state"], string> = {
  idle: "待机",
  connecting: "正在连接",
  listening: "正在转写",
  stopping: "正在结束",
  error: "连接中断",
};

const desktop = isTauri();

function IconButton({
  label,
  children,
  onClick,
  active,
  disabled,
  className = "",
}: {
  label: string;
  children: ReactNode;
  onClick: () => void;
  active?: boolean;
  disabled?: boolean;
  className?: string;
}) {
  return (
    <button
      type="button"
      className={`icon-button ${className}`}
      title={label}
      aria-label={label}
      aria-pressed={active}
      disabled={disabled}
      onClick={onClick}
    >
      {children}
    </button>
  );
}

function Modal({
  title,
  children,
  onClose,
}: {
  title: string;
  children: ReactNode;
  onClose: () => void;
}) {
  const dialog = useRef<HTMLDialogElement>(null);
  useEffect(() => {
    const element = dialog.current;
    element?.showModal();
    return () => element?.close();
  }, []);
  return (
    <dialog
      ref={dialog}
      aria-label={title}
      onCancel={(event) => {
        event.preventDefault();
        onClose();
      }}
    >
      <header className="modal-heading">
        <h2>{title}</h2>
        <IconButton label="取消" onClick={onClose}>
          <X size={16} />
        </IconButton>
      </header>
      {children}
    </dialog>
  );
}

function App() {
  const [captions, setCaptions] = useState(createCaptionState);
  const [status, setStatus] = useState<SessionStatus>({ state: "idle" });
  const [settings, setSettings] = useState(defaults);
  const [settingsDraft, setSettingsDraft] = useState(defaults);
  const [terms, setTerms] = useState(defaults.terms.join("\n"));
  const [key, setKey] = useState("");
  const [keys, setKeys] = useState<Partial<Record<Provider, boolean>>>({});
  const [settingsLoaded, setSettingsLoaded] = useState(false);
  const [sessionProvider, setSessionProvider] = useState<Provider | null>(null);
  const [usage, setUsage] = useState<SessionUsage | null>(null);
  const [sessionTerms, setSessionTerms] = useState<SessionTerms | null>(null);
  const [showUsage, setShowUsage] = useState(false);
  const [ready, setReady] = useState(false);
  const [view, setView] = useState<View>("captions");
  const [historyTab, setHistoryTab] = useState<"current" | "saved">("current");
  const [showOriginal, setShowOriginal] = useState(true);
  const [prompt, setPrompt] = useState<Prompt>(null);
  const [demo, setDemo] = useState(false);
  const [demoSession, setDemoSession] = useState(false);
  const [error, setError] = useState("");
  const [notice, setNotice] = useState("");
  const [translationWarning, setTranslationWarning] = useState("");
  const [transcripts, setTranscripts] = useState<Transcript[]>([]);
  const [historyLoading, setHistoryLoading] = useState(false);
  const [selected, setSelected] = useState<Transcript | null>(null);
  const [savedContent, setSavedContent] = useState("");
  const [edit, setEdit] = useState<Edit | null>(null);
  const [deleteItem, setDeleteItem] = useState<Transcript | null>(null);
  const [busy, setBusy] = useState(false);
  const [knowledgeGenerating, setKnowledgeGenerating] = useState(false);
  const demoTimer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const localSpeakerNumber = useRef(0);
  const confirmClose = useRef(false);
  const activeSession = useRef<{ id: string; provider: Provider } | null>(null);
  const expanded =
    view !== "captions" || Boolean(prompt || edit || deleteItem || showUsage);
  const displayFontSize = demoSession
    ? settingsDraft.fontSize
    : settings.fontSize;
  const compactHeight = 204 + Math.max(0, displayFontSize - 24) * 3;
  const returnToBottom = useCaptionWindow(expanded, compactHeight, setError);

  const live =
    status.state === "connecting" ||
    status.state === "listening" ||
    status.state === "stopping";
  const confirmedContent = exportCaptions(captions);
  const dirty = Boolean(
    !demoSession && confirmedContent && confirmedContent !== savedContent,
  );
  const anyCaption =
    captions.confirmed.length > 0 || captions.drafts.length > 0;
  const currentProvider = sessionProvider ?? settings.provider;
  const sessionUsage = usage?.session_id === captions.sessionId ? usage : null;
  const learnedTerms =
    sessionTerms?.session_id === captions.sessionId
      ? sessionTerms.candidates
      : [];
  const providerName = providerNames[currentProvider];
  const selectedProviderName = providerNames[settingsDraft.provider];
  const selectedHasKey = keys[settingsDraft.provider];

  function reportError(reason: unknown) {
    setError(reason instanceof Error ? reason.message : String(reason));
  }

  useEffect(() => {
    if (!desktop) return;
    let disposed = false;
    const subscriptions = [
      listen<SessionStatus>("session-status", ({ payload }) => {
        if (disposed) return;
        setStatus(payload);
        if (payload.state === "error")
          setError(payload.message || "实时字幕连接中断。");
        if (payload.state === "idle" || payload.state === "error")
          setCaptions(clearCaptionDrafts);
      }),
      getCurrentWindow().onCloseRequested((event) => {
        event.preventDefault();
        if (disposed) return;
        if (confirmClose.current) setPrompt("close");
        else void getCurrentWindow().destroy().catch(reportError);
      }),
      listen<TranscriptFrame>("transcript", ({ payload }) => {
        if (!disposed)
          setCaptions((current) => applyCaptionFrame(current, payload));
      }),
      listen<string>("translation-warning", ({ payload }) => {
        if (!disposed) setTranslationWarning(payload);
      }),
      listen<SessionUsage>("session-usage", ({ payload }) => {
        const session = activeSession.current;
        if (
          !disposed &&
          session?.id === payload.session_id &&
          session.provider === payload.provider
        ) {
          setUsage(payload);
        }
      }),
      listen<SessionTerms>("session-terms", ({ payload }) => {
        if (!disposed && activeSession.current?.id === payload.session_id)
          setSessionTerms(payload);
      }),
    ];
    void Promise.all(subscriptions)
      .then(() => {
        if (!disposed) setReady(true);
      })
      .catch((reason) => {
        if (!disposed) reportError(reason);
      });
    void invoke<Settings>("load_settings")
      .then((loaded) => {
        if (disposed) return;
        setSettings(loaded);
        setSettingsLoaded(true);
        setSettingsDraft(loaded);
        setTerms(loaded.terms.join("\n"));
        return getCurrentWindow().setAlwaysOnTop(loaded.alwaysOnTop);
      })
      .catch((reason) => {
        if (!disposed) reportError(reason);
      });
    for (const provider of ["openai", "soniox"] as const) {
      void invoke<boolean>("has_api_key", { provider })
        .then((exists) => {
          if (!disposed)
            setKeys((current) =>
              current[provider] === undefined
                ? { ...current, [provider]: exists }
                : current,
            );
        })
        .catch((reason) => {
          if (!disposed) reportError(reason);
        });
    }
    return () => {
      disposed = true;
      for (const subscription of subscriptions)
        void subscription.then((unlisten) => unlisten()).catch(() => {});
    };
  }, []);

  useLayoutEffect(() => {
    confirmClose.current = dirty || live || knowledgeGenerating;
  }, [dirty, live, knowledgeGenerating]);

  useEffect(
    () => () => {
      if (demoTimer.current !== null) clearTimeout(demoTimer.current);
    },
    [],
  );

  function resetSession(isDemo: boolean, nextCaptions = createCaptionState()) {
    setCaptions(nextCaptions);
    setSavedContent("");
    setDemoSession(isDemo);
    setUsage(null);
    setSessionTerms(null);
    setShowUsage(false);
    setNotice("");
    setTranslationWarning("");
    setError("");
    setView("captions");
  }

  async function startLive() {
    const previousActiveSession = activeSession.current;
    setPrompt(null);
    setStatus({ state: "connecting" });
    setError("");
    try {
      // Native transcript events can arrive before the command resolves.
      const nextCaptions = createCaptionState();
      const provider = settings.provider;
      resetSession(false, nextCaptions);
      activeSession.current = { id: nextCaptions.sessionId, provider };
      setSessionProvider(provider);
      await invoke("start_session", {
        consent: true,
        provider,
        sessionId: nextCaptions.sessionId,
      });
    } catch (reason) {
      setCaptions(captions);
      setSavedContent(savedContent);
      setDemoSession(demoSession);
      setSessionProvider(sessionProvider);
      activeSession.current = previousActiveSession;
      setUsage(usage);
      setSessionTerms(sessionTerms);
      setStatus({ state: "error" });
      reportError(reason);
    }
  }

  async function stopLive() {
    setStatus({ state: "stopping" });
    try {
      await invoke("stop_session");
    } catch (reason) {
      setStatus({ state: "error" });
      reportError(reason);
    }
  }

  function stopDemo() {
    if (demoTimer.current !== null) clearTimeout(demoTimer.current);
    demoTimer.current = null;
    setDemo(false);
    setCaptions(clearCaptionDrafts);
  }

  function startDemo() {
    activeSession.current = null;
    setSessionProvider(null);
    setPrompt(null);
    resetSession(true);
    setStatus({ state: "idle" });
    setDemo(true);
    const batches = [...demoTokenBatches, longPreview];
    let position = 0;
    function next() {
      if (position === batches.length) {
        stopDemo();
        return;
      }
      const tokens = batches[position++];
      setCaptions((current) => applyCaptionTokens(current, tokens));
      demoTimer.current = setTimeout(next, 1500);
    }
    next();
  }

  async function updateSettings(updated: Settings) {
    if (desktop) await invoke("save_settings", { settings: updated });
    setSettings(updated);
    setSettingsLoaded(true);
  }

  async function togglePin() {
    const updated = { ...settings, alwaysOnTop: !settings.alwaysOnTop };
    setBusy(true);
    try {
      await getCurrentWindow().setAlwaysOnTop(updated.alwaysOnTop);
      setSettings(updated);
      await invoke("save_settings", { settings: updated });
    } catch (reason) {
      reportError(reason);
    } finally {
      setBusy(false);
    }
  }

  async function saveSettings(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    setBusy(true);
    setError("");
    try {
      const updated = {
        ...settings,
        fontSize: settingsDraft.fontSize,
        provider: settingsDraft.provider,
        model:
          settingsDraft.provider === "soniox"
            ? settingsDraft.model.trim()
            : settings.model,
        terms: terms
          .split(/\r?\n/)
          .map((term) => term.trim())
          .filter(Boolean),
      };
      await updateSettings(updated);
      setSettingsDraft(updated);
      setTerms(updated.terms.join("\n"));
      setNotice(desktop ? "设置已保存" : "设置已应用到当前预览");
    } catch (reason) {
      reportError(reason);
    } finally {
      setBusy(false);
    }
  }

  async function keepTerm(term: string) {
    if (
      settings.terms.some((saved) => saved.toLowerCase() === term.toLowerCase())
    )
      return;
    setBusy(true);
    setError("");
    try {
      await updateSettings({ ...settings, terms: [...settings.terms, term] });
      setTerms((draft) => {
        const exists = draft
          .split(/\r?\n/)
          .some((line) => line.trim().toLowerCase() === term.toLowerCase());
        return exists
          ? draft
          : [draft.trimEnd(), term].filter(Boolean).join("\n");
      });
      setNotice(`已保留“${term}”，下次会话将使用这个术语。`);
    } catch (reason) {
      reportError(reason);
    } finally {
      setBusy(false);
    }
  }

  async function dismissTerm(term: string) {
    const sessionId = activeSession.current?.id;
    if (!sessionId) return;
    setBusy(true);
    setError("");
    try {
      await invoke("dismiss_learned_term", { sessionId, term });
    } catch (reason) {
      reportError(reason);
    } finally {
      setBusy(false);
    }
  }

  async function changeKey(remove: boolean) {
    setBusy(true);
    setError("");
    const provider = settingsDraft.provider;
    const name = providerNames[provider];
    try {
      if (remove) await invoke("delete_api_key", { provider });
      else await invoke("save_api_key", { provider, key: key.trim() });
      setKey("");
      setKeys((current) => ({ ...current, [provider]: !remove }));
      setNotice(
        remove ? `${name} 密钥已移除` : `${name} 密钥已保存到系统安全凭据存储`,
      );
    } catch (reason) {
      reportError(reason);
    } finally {
      setBusy(false);
    }
  }

  async function saveCurrent() {
    setBusy(true);
    try {
      await invoke<number>("save_transcript", { content: confirmedContent });
      setSavedContent(confirmedContent);
      setNotice("已确认的字幕已保存到本机");
    } catch (reason) {
      reportError(reason);
    } finally {
      setBusy(false);
    }
  }

  async function openHistory() {
    setKey("");
    setSelected(null);
    setHistoryTab("current");
    setView("history");
    if (!desktop) return;
    setHistoryLoading(true);
    try {
      setTranscripts(await invoke<Transcript[]>("list_transcripts"));
    } catch (reason) {
      reportError(reason);
    } finally {
      setHistoryLoading(false);
    }
  }

  async function removeTranscript(item: Transcript) {
    setBusy(true);
    try {
      await invoke("delete_transcript", { id: item.id });
      setTranscripts((current) => current.filter(({ id }) => id !== item.id));
      setSelected(null);
      setDeleteItem(null);
      setNotice("已删除记录");
    } catch (reason) {
      reportError(reason);
    } finally {
      setBusy(false);
    }
  }

  function closeWindow() {
    setPrompt(null);
    void getCurrentWindow().destroy().catch(reportError);
  }

  function requestLive() {
    if (!keys[settings.provider]) {
      openSettings();
      setNotice(
        `请先在设置里保存 ${providerNames[settings.provider]} API 密钥。`,
      );
      return;
    }
    setPrompt("live");
  }

  function openEdit(segment: CaptionSegment) {
    setCaptions((current) => freezeCaptionSegment(current, segment.id));
    setEdit({
      segment,
      text: segment.text,
      speaker: segment.speaker ?? "",
      name: speakerLabel(captions, segment.speaker),
      newSpeaker: "",
    });
  }

  function applyEdit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    if (!edit) return;
    const isNew = edit.speaker === addSpeaker;
    if (isNew && !edit.newSpeaker.trim()) return;
    const speaker = isNew
      ? `local:${++localSpeakerNumber.current}`
      : edit.speaker;
    setCaptions((current) => {
      const corrected = editCaptionSegment(current, edit.segment.id, {
        text: edit.text.trim(),
        baseText: edit.segment.text,
        speaker: speaker || null,
      });
      return isNew
        ? renameSpeaker(corrected, speaker, edit.newSpeaker.trim())
        : corrected;
    });
    setEdit(null);
  }

  function openSettings() {
    setKey("");
    setSettingsDraft(settings);
    setTerms(settings.terms.join("\n"));
    setView("settings");
  }

  function exitDemo() {
    stopDemo();
    resetSession(false);
  }

  function back() {
    setKey("");
    setView("captions");
  }

  return (
    <main
      className={`livecap ${expanded ? "expanded" : "compact"}`}
      style={{ "--caption-size": `${displayFontSize}px` } as CSSProperties}
    >
      <header className="titlebar">
        <div
          className="drag-region"
          onMouseDown={(event) => {
            if (!desktop || event.button !== 0) return;
            event.preventDefault();
            void getCurrentWindow().startDragging().catch(reportError);
          }}
        >
          <span className="app-mark">
            <Captions size={17} strokeWidth={2.1} />
          </span>
          <strong>Livecap</strong>
          <span className="title-divider" />
          <span
            className={`status ${demoSession ? "demo" : status.state}`}
            role="status"
          >
            <span className="status-dot" />
            {demoSession
              ? demo
                ? "预览播放中"
                : "样式预览"
              : statusLabel[status.state]}
          </span>
        </div>
        <div className="title-actions">
          <IconButton
            label={settings.alwaysOnTop ? "取消置顶" : "窗口置顶"}
            active={settings.alwaysOnTop && desktop}
            disabled={!desktop || !settingsLoaded || busy}
            onClick={() => void togglePin()}
          >
            <Pin size={15} />
          </IconButton>
          {view === "captions" && (
            <IconButton
              label="回到屏幕底部"
              disabled={!desktop}
              onClick={returnToBottom}
            >
              <ArrowDownToLine size={15} />
            </IconButton>
          )}
          <IconButton
            label="字幕记录"
            active={view === "history"}
            onClick={() => (view === "history" ? back() : void openHistory())}
          >
            <History size={16} />
          </IconButton>
          <IconButton
            label="设置"
            active={view === "settings"}
            onClick={() => (view === "settings" ? back() : openSettings())}
          >
            <Settings2 size={16} />
          </IconButton>
          <span className="title-divider" />
          <IconButton
            label="最小化"
            disabled={!desktop}
            onClick={() =>
              void getCurrentWindow().minimize().catch(reportError)
            }
          >
            <Minus size={16} />
          </IconButton>
          <IconButton
            label="关闭"
            disabled={!desktop}
            className="close-window"
            onClick={() =>
              confirmClose.current ? setPrompt("close") : closeWindow()
            }
          >
            <X size={17} />
          </IconButton>
        </div>
      </header>

      {error && (
        <div className="banner error" role="alert">
          <CircleAlert size={16} />
          <span>{error}</span>
          <IconButton label="关闭错误提示" onClick={() => setError("")}>
            <X size={14} />
          </IconButton>
        </div>
      )}
      {translationWarning && (
        <div className="banner warning" role="status">
          <CircleAlert size={15} />
          <span>{translationWarning}</span>
          <IconButton
            label="关闭翻译提示"
            onClick={() => setTranslationWarning("")}
          >
            <X size={14} />
          </IconButton>
        </div>
      )}
      {notice && (
        <div className="banner notice" role="status">
          <Check size={15} />
          <span>{notice}</span>
          <IconButton label="关闭提示" onClick={() => setNotice("")}>
            <X size={14} />
          </IconButton>
        </div>
      )}

      {view === "captions" && (
        <>
          <div className="caption-region">
            {anyCaption ? (
              <CompactCaptions
                captions={captions}
                showOriginal={showOriginal}
              />
            ) : (
              <p className="caption-placeholder">
                {status.state === "connecting"
                  ? `正在准备 ${providerName} 转写…`
                  : status.state === "stopping"
                    ? "正在结束转写…"
                    : status.state === "listening"
                      ? "等待系统播放声音…"
                      : desktop
                        ? "开始后，字幕会在这里显示。"
                        : "浏览器预览 · 请在设置中打开样式预览。"}
              </p>
            )}
          </div>
          <footer className="toolbar">
            {demoSession ? (
              <button
                className="button subtle"
                type="button"
                onClick={exitDemo}
              >
                退出预览
              </button>
            ) : live ? (
              <button
                className="button subtle stop"
                type="button"
                disabled={status.state === "stopping"}
                onClick={() => void stopLive()}
              >
                <Square size={12} fill="currentColor" />
                {status.state === "stopping" ? "正在结束…" : "停止字幕"}
              </button>
            ) : (
              <button
                className="button primary"
                type="button"
                disabled={
                  !desktop ||
                  !ready ||
                  !settingsLoaded ||
                  busy ||
                  knowledgeGenerating
                }
                onClick={requestLive}
              >
                <Play size={12} fill="currentColor" />
                开始字幕
              </button>
            )}
            <div className="signal-strip">
              {demoSession ? (
                <span className="demo-note">
                  样式预览 · 不联网 · 不产生 API 费用
                </span>
              ) : (
                <span className="signal-source">
                  系统播放声音 <span aria-hidden="true">·</span>{" "}
                  <strong>{providerName}</strong>
                </span>
              )}
              {(status.state === "listening" || sessionProvider) && (
                <div className="signal-end">
                  {status.state === "listening" && (
                    <AudioMeter onError={setError} />
                  )}
                  {sessionProvider && (
                    <button
                      type="button"
                      className="usage-button"
                      aria-haspopup="dialog"
                      onClick={() => setShowUsage(true)}
                    >
                      {sessionUsage?.estimated_usd != null
                        ? `${sessionUsage.incomplete ? "已知费用估算" : "本次预估"} ${formatUsd(sessionUsage.estimated_usd)}`
                        : sessionProvider === "openai" && live && !sessionUsage
                          ? "正在估算费用…"
                          : "费用暂不可估"}
                      <ChevronUp size={13} />
                    </button>
                  )}
                </div>
              )}
            </div>
          </footer>
        </>
      )}

      <section className="panel" hidden={view !== "settings"}>
        <header className="panel-heading">
          <IconButton label="返回字幕" onClick={back}>
            <ArrowLeft size={17} />
          </IconButton>
          <h1>设置</h1>
        </header>
        <div className="panel-scroll">
          <KnowledgeSettings
            active={view === "settings"}
            desktop={desktop}
            canGenerate={!live && !busy}
            hasKey={keys.openai === true}
            onGenerationChange={setKnowledgeGenerating}
            Modal={Modal}
            formatUsd={formatUsd}
            displayDate={displayDate}
          />
          <form className="settings-form" onSubmit={saveSettings}>
            <div className="field">
              <label htmlFor="provider">转写与翻译服务</label>
              <select
                id="provider"
                value={settingsDraft.provider}
                disabled={busy || live}
                onChange={(event) => {
                  const provider =
                    event.target.value === "soniox" ? "soniox" : "openai";
                  setKey("");
                  setSettingsDraft({ ...settingsDraft, provider });
                }}
              >
                <option value="openai">OpenAI</option>
                <option value="soniox">Soniox</option>
              </select>
            </div>
            <div className="field">
              <label htmlFor="font-size">
                中文字幕字号 <output>{settingsDraft.fontSize} px</output>
              </label>
              <input
                id="font-size"
                type="range"
                min={18}
                max={36}
                value={settingsDraft.fontSize}
                onChange={(event) =>
                  setSettingsDraft({
                    ...settingsDraft,
                    fontSize: Number(event.target.value),
                  })
                }
              />
            </div>
            {settingsDraft.provider === "openai" && (
              <p className="wide provider-detail">
                gpt-live-transcribe 实时转录英文，gpt-6-luna
                生成并修订中文字幕。说话人需手动标记。
              </p>
            )}
            {settingsDraft.provider === "soniox" && (
              <div className="field">
                <label htmlFor="model">Soniox 模型</label>
                <input
                  id="model"
                  value={settingsDraft.model}
                  required
                  onChange={(event) =>
                    setSettingsDraft({
                      ...settingsDraft,
                      model: event.target.value,
                    })
                  }
                />
              </div>
            )}
            <details className="wide advanced-terms">
              <summary>补充或纠正术语（可选）</summary>
              <section
                className="learned-terms"
                aria-labelledby="learned-heading"
              >
                <h2 id="learned-heading">自动学习的术语（仅本次）</h2>
                {sessionProvider === "openai" ||
                (!sessionProvider && settingsDraft.provider === "openai") ? (
                  <p>
                    自动发现重复出现的缩写和技术词。本次用于翻译，保留后下次用于识别。
                  </p>
                ) : (
                  <p>自动学习术语适用于 OpenAI 会话。</p>
                )}
                {learnedTerms.length > 0 ? (
                  <ul className="term-candidates">
                    {learnedTerms.map(({ term, occurrences, active }) => {
                      const saved = settings.terms.some(
                        (item) => item.toLowerCase() === term.toLowerCase(),
                      );
                      return (
                        <li key={term}>
                          <div>
                            <strong>{term}</strong>{" "}
                            <span>
                              出现 {occurrences} 次 ·{" "}
                              {active ? "翻译中使用" : "候选"}
                            </span>
                          </div>
                          <button
                            className="button subtle"
                            type="button"
                            disabled={!desktop || busy || saved}
                            aria-label={`保留 ${term} 为固定术语`}
                            onClick={() => void keepTerm(term)}
                          >
                            {saved ? "已保留" : "保留"}
                          </button>
                          <button
                            className="button subtle"
                            type="button"
                            disabled={!desktop || busy}
                            aria-label={`忽略本次会话的术语 ${term}`}
                            onClick={() => void dismissTerm(term)}
                          >
                            忽略
                          </button>
                        </li>
                      );
                    })}
                  </ul>
                ) : (
                  (sessionProvider === "openai" ||
                    (!sessionProvider &&
                      settingsDraft.provider === "openai")) && (
                    <p>本次会话暂无候选术语。</p>
                  )
                )}
              </section>
              <div className="field">
                <label htmlFor="terms">
                  固定术语（可选） <span>每行一个，留空也可以开始字幕</span>
                </label>
                <textarea
                  id="terms"
                  rows={3}
                  value={terms}
                  onChange={(event) => setTerms(event.target.value)}
                  placeholder="在这里添加术语"
                />
              </div>
            </details>
            <div className="wide form-actions">
              <span className="history-hint">
                服务、模型和术语在下次会话开始时生效
              </span>
              <button
                className="button primary"
                type="submit"
                disabled={
                  busy ||
                  (settingsDraft.provider === "soniox" &&
                    !settingsDraft.model.trim())
                }
              >
                保存设置
              </button>
            </div>
          </form>
          <div className="credential">
            <label htmlFor="key">
              {selectedProviderName} API 密钥{" "}
              <span>
                {!desktop
                  ? "仅桌面端可用"
                  : selectedHasKey === undefined
                    ? "状态未知"
                    : selectedHasKey
                      ? "已配置"
                      : "未配置"}
              </span>
            </label>
            <p>
              各服务的密钥分别保存在系统安全凭据存储中。
              {settingsDraft.provider !== settings.provider &&
                "保存设置后才会切换服务。"}
            </p>
            <div className="credential-input">
              <input
                id="key"
                type="password"
                autoComplete="off"
                value={key}
                disabled={!desktop || live || busy}
                onChange={(event) => setKey(event.target.value)}
                placeholder={
                  selectedHasKey
                    ? "输入新密钥以替换"
                    : `输入 ${selectedProviderName} API 密钥`
                }
              />
              <button
                className="button subtle"
                type="button"
                disabled={!desktop || live || busy || !key.trim()}
                onClick={() => void changeKey(false)}
              >
                保存密钥
              </button>
              {selectedHasKey && (
                <button
                  className="button subtle"
                  type="button"
                  disabled={live || busy}
                  onClick={() => void changeKey(true)}
                >
                  移除
                </button>
              )}
            </div>
          </div>
          <div className="settings-preview">
            <label className="check-label">
              <input
                type="checkbox"
                checked={showOriginal}
                onChange={(event) => setShowOriginal(event.target.checked)}
              />
              在字幕条显示英文原文
            </label>
            <button
              className="button subtle"
              type="button"
              disabled={live || busy}
              onClick={() =>
                demoSession ? back() : dirty ? setPrompt("demo") : startDemo()
              }
            >
              <FlaskConical size={14} />
              {demoSession ? "返回样式预览" : "样式预览（不联网）"}
            </button>
          </div>
        </div>
      </section>

      {view === "history" && (
        <section className="panel">
          <header className="panel-heading">
            <IconButton
              label={selected ? "返回记录列表" : "返回字幕"}
              onClick={() => (selected ? setSelected(null) : back())}
            >
              <ArrowLeft size={17} />
            </IconButton>
            <h1>{selected ? "字幕记录" : "记录"}</h1>
            {!selected && historyTab === "current" && (
              <>
                {captions.droppedSegments > 0 && (
                  <span className="history-hint">
                    {captions.droppedSegments} 段早期字幕已移除
                  </span>
                )}
                <button
                  className="button subtle"
                  type="button"
                  disabled={!desktop || demoSession || !dirty || busy}
                  onClick={() => void saveCurrent()}
                >
                  <Save size={15} />
                  保存已确认字幕
                </button>
              </>
            )}
            {selected && (
              <IconButton
                label="删除记录"
                onClick={() => setDeleteItem(selected)}
              >
                <Trash2 size={16} />
              </IconButton>
            )}
          </header>
          {!selected && (
            <div className="history-tabs" aria-label="字幕记录范围">
              <button
                type="button"
                aria-pressed={historyTab === "current"}
                onClick={() => setHistoryTab("current")}
              >
                当前会话
              </button>
              <button
                type="button"
                aria-pressed={historyTab === "saved"}
                onClick={() => setHistoryTab("saved")}
              >
                已保存
              </button>
            </div>
          )}
          {!selected && historyTab === "current" ? (
            anyCaption ? (
              <div className="caption-grid records-grid">
                <CaptionLane
                  lane="translation"
                  captions={captions}
                  onEdit={openEdit}
                />
                <CaptionLane
                  lane="original"
                  captions={captions}
                  onEdit={openEdit}
                />
              </div>
            ) : (
              <p className="history-empty records-empty">
                当前会话还没有字幕。
              </p>
            )
          ) : (
            <div className="panel-scroll">
              {selected ? (
                <pre className="history-content">{selected.content}</pre>
              ) : transcripts.length > 0 ? (
                <ul className="history-list">
                  {transcripts.map((item) => (
                    <li key={item.id}>
                      <button
                        className="history-entry"
                        type="button"
                        onClick={() => setSelected(item)}
                      >
                        <strong>
                          {item.content.split("\n").find((line) => line.trim())}
                        </strong>
                        <span>{displayDate(item.createdAt)}</span>
                      </button>
                      <IconButton
                        label="删除记录"
                        onClick={() => setDeleteItem(item)}
                      >
                        <Trash2 size={15} />
                      </IconButton>
                    </li>
                  ))}
                </ul>
              ) : (
                <p className="history-empty">
                  {historyLoading
                    ? "正在读取…"
                    : desktop
                      ? "还没有手动保存的字幕。"
                      : "在桌面端可以保存和查看字幕记录。"}
                </p>
              )}
            </div>
          )}
        </section>
      )}

      {showUsage && sessionProvider && (
        <UsageDetails
          provider={sessionProvider}
          usage={sessionUsage}
          onClose={() => setShowUsage(false)}
        />
      )}

      {prompt && (
        <Modal
          title={
            prompt === "live"
              ? "允许发送播放音频？"
              : prompt === "demo"
                ? "打开样式预览？"
                : "关闭 Livecap？"
          }
          onClose={() => setPrompt(null)}
        >
          {prompt === "live" && (
            <p>
              Livecap 会把系统播放设备上的音频发送给{" "}
              {providerNames[settings.provider]}
              ，其中可能包含其他应用的声音和系统通知。
              {settings.provider === "openai"
                ? "Livecap 也会把英文草稿、最终文字、有限的前文和你设置的术语发送给 OpenAI，用于生成和修订中文翻译。"
                : "Soniox 会返回转写原文和中文翻译。"}
              音频不会保存在本机，字幕仅在你手动保存时才写入本机。
            </p>
          )}
          {prompt === "demo" && (
            <p>样式预览使用示例字幕，不联网，也不采集音频。</p>
          )}
          {prompt === "close" && live && (
            <p>关闭窗口会结束当前实时字幕会话。</p>
          )}
          {prompt === "close" && knowledgeGenerating && (
            <p>关闭窗口会中断知识缓存生成。已保存的旧缓存会保留。</p>
          )}
          {dirty && (
            <p className="warning">
              当前会话有未保存的字幕，继续会丢弃这些文字。
            </p>
          )}
          <div className="modal-actions">
            <button
              className="button subtle"
              type="button"
              autoFocus
              onClick={() => setPrompt(null)}
            >
              取消
            </button>
            <button
              className="button primary"
              type="button"
              onClick={() => {
                if (prompt === "live") void startLive();
                else if (prompt === "demo") startDemo();
                else closeWindow();
              }}
            >
              {prompt === "live"
                ? "同意并开始"
                : prompt === "demo"
                  ? "打开预览"
                  : "关闭窗口"}
            </button>
          </div>
        </Modal>
      )}

      {edit && (
        <Modal title="编辑字幕" onClose={() => setEdit(null)}>
          <form className="edit-form" onSubmit={applyEdit}>
            {edit.segment.speaker && (
              <div className="field">
                <label htmlFor="speaker-name">此说话人的会话昵称</label>
                <div className="credential-input">
                  <input
                    id="speaker-name"
                    value={edit.name}
                    onChange={(event) =>
                      setEdit({ ...edit, name: event.target.value })
                    }
                  />
                  <button
                    className="button subtle"
                    type="button"
                    disabled={!edit.name.trim()}
                    onClick={() => {
                      const speaker = edit.segment.speaker;
                      if (speaker)
                        setCaptions((current) =>
                          renameSpeaker(current, speaker, edit.name.trim()),
                        );
                      setEdit(null);
                    }}
                  >
                    重命名全部
                  </button>
                </div>
              </div>
            )}
            <div className="field">
              <label htmlFor="segment-speaker">本条字幕的说话人</label>
              <select
                id="segment-speaker"
                value={edit.speaker}
                onChange={(event) =>
                  setEdit({ ...edit, speaker: event.target.value })
                }
              >
                <option value="">说话人未识别</option>
                {[
                  ...new Set([
                    ...Object.keys(captions.speakerNames),
                    ...[...captions.confirmed, ...captions.drafts].flatMap(
                      ({ speaker }) => (speaker ? [speaker] : []),
                    ),
                  ]),
                ].map((speaker) => (
                  <option key={speaker} value={speaker}>
                    {speakerLabel(captions, speaker)}
                  </option>
                ))}
                <option value={addSpeaker}>新增说话人…</option>
              </select>
            </div>
            {edit.speaker === addSpeaker && (
              <div className="field">
                <label htmlFor="new-speaker">新说话人的会话昵称</label>
                <input
                  id="new-speaker"
                  value={edit.newSpeaker}
                  required
                  onChange={(event) =>
                    setEdit({ ...edit, newSpeaker: event.target.value })
                  }
                />
              </div>
            )}
            <div className="field">
              <label htmlFor="segment-text">
                {edit.segment.lane === "translation" ? "中文字幕" : "英文原文"}
              </label>
              <textarea
                id="segment-text"
                rows={3}
                value={edit.text}
                required
                onChange={(event) =>
                  setEdit({ ...edit, text: event.target.value })
                }
              />
            </div>
            <div className="modal-actions">
              <button
                className="button subtle"
                type="button"
                onClick={() => setEdit(null)}
              >
                取消
              </button>
              <button
                className="button primary"
                type="submit"
                disabled={
                  !edit.text.trim() ||
                  (edit.speaker === addSpeaker && !edit.newSpeaker.trim())
                }
              >
                保存本条
              </button>
            </div>
          </form>
        </Modal>
      )}

      {deleteItem && (
        <Modal
          title="删除这条记录？"
          onClose={() => {
            if (!busy) setDeleteItem(null);
          }}
        >
          <p>这条字幕记录将从本机删除。</p>
          <div className="modal-actions">
            <button
              className="button subtle"
              type="button"
              autoFocus
              disabled={busy}
              onClick={() => setDeleteItem(null)}
            >
              取消
            </button>
            <button
              className="button danger"
              type="button"
              disabled={busy}
              onClick={() => void removeTranscript(deleteItem)}
            >
              删除记录
            </button>
          </div>
        </Modal>
      )}
    </main>
  );
}

const usd = new Intl.NumberFormat("en-US", {
  style: "currency",
  currency: "USD",
  minimumFractionDigits: 4,
  maximumFractionDigits: 4,
});

function formatUsd(value: number | null) {
  if (value === null) return "暂不可估";
  return value > 0 && value < 0.0001 ? "< $0.0001" : usd.format(value);
}

function UsageDetails({
  provider,
  usage,
  onClose,
}: {
  provider: Provider;
  usage: SessionUsage | null;
  onClose: () => void;
}) {
  const openAI = provider === "openai";
  return (
    <Modal title="本次会话费用估算" onClose={onClose}>
      <dl className="usage-breakdown">
        <div>
          <dt>转录</dt>
          <dd>{formatUsd(usage?.transcription_usd ?? null)}</dd>
        </div>
        <div>
          <dt>翻译</dt>
          <dd>{formatUsd(usage?.translation_usd ?? null)}</dd>
        </div>
        <div className="usage-total">
          <dt>{usage?.incomplete ? "已知部分合计" : "合计"}</dt>
          <dd>{formatUsd(usage?.estimated_usd ?? null)}</dd>
        </div>
      </dl>
      {openAI && usage && (
        <p className="usage-basis">
          转录按已发送音频计时（
          {(usage.audio_ms / 1000).toLocaleString("zh-CN", {
            maximumFractionDigits: 1,
          })}{" "}
          秒）。 翻译按 API 返回的用量计算，输入{" "}
          {usage.input_tokens.toLocaleString("zh-CN")} token（其中缓存{" "}
          {usage.cached_input_tokens.toLocaleString("zh-CN")}）， 输出{" "}
          {usage.output_tokens.toLocaleString("zh-CN")} token。
        </p>
      )}
      {!openAI && <p>当前 Soniox 会话没有可用于估价的用量数据。</p>}
      {!!usage?.pending_translation_requests && (
        <p>
          还有 {usage.pending_translation_requests} 次翻译请求在等待用量回报。
        </p>
      )}
      {usage?.incomplete && (
        <p className="warning">部分用量未取得，实际费用可能更高。</p>
      )}
      <p>
        金额按 API
        价格估算，实际扣费以账单为准。估算不抵扣免费额度，也不会查询账户余额。
      </p>
    </Modal>
  );
}

function AudioMeter({ onError }: { onError: (message: string) => void }) {
  const [level, setLevel] = useState<number | null>(null);
  useEffect(() => {
    let disposed = false;
    let expire: ReturnType<typeof setTimeout> | undefined;
    const subscription = listen<number>("audio-level", ({ payload }) => {
      if (disposed || !Number.isFinite(payload)) return;
      setLevel(Math.min(1, Math.max(0, payload)));
      clearTimeout(expire);
      expire = setTimeout(() => setLevel(null), 500);
    });
    void subscription.catch(() => {
      if (!disposed) onError("无法读取音频电平。字幕会话仍可继续。");
    });
    return () => {
      disposed = true;
      clearTimeout(expire);
      void subscription.then((unlisten) => unlisten()).catch(() => {});
    };
  }, [onError]);

  return (
    <div className="audio-meter">
      <span>输入电平</span>
      {level === null ? (
        <span className="meter-waiting">等待音频</span>
      ) : (
        <meter
          min={0}
          max={1}
          value={level}
          aria-label="系统播放音频输入电平"
        />
      )}
    </div>
  );
}

function TailCaption({
  text,
  language,
}: {
  text: string;
  language: "zh-Hans" | "en";
}) {
  const viewport = useRef<HTMLDivElement>(null);
  useLayoutEffect(() => {
    const element = viewport.current;
    if (element) element.scrollTop = element.scrollHeight;
  }, [text]);
  useEffect(() => {
    const element = viewport.current;
    if (!element) return;
    const observer = new ResizeObserver(() => {
      element.scrollTop = element.scrollHeight;
    });
    observer.observe(element);
    if (element.firstElementChild) observer.observe(element.firstElementChild);
    return () => observer.disconnect();
  }, []);
  return (
    <div className="tail-caption" ref={viewport}>
      <p lang={language}>{text.trim()}</p>
    </div>
  );
}

function CompactCaptions({
  captions,
  showOriginal,
}: {
  captions: CaptionState;
  showOriginal: boolean;
}) {
  const { target, matchedSource, latestSource, sourceInProgress } =
    selectLiveCaptions(captions);
  const source = matchedSource ?? latestSource;

  return (
    <div
      className={`caption-strip ${showOriginal ? "" : "translation-only"}`}
      aria-label="最新字幕"
    >
      <div className="strip-translation">
        {target ? (
          <TailCaption text={target.segment.text} language="zh-Hans" />
        ) : (
          <p className="strip-placeholder">等待中文翻译…</p>
        )}
        {target?.draft && <span className="strip-revising">修订中</span>}
      </div>
      {showOriginal && (
        <div
          className={`strip-originals ${sourceInProgress ? "split-originals" : ""}`}
        >
          {source && (
            <div className="strip-original-row">
              {target && !matchedSource && <span>实时原文</span>}
              <TailCaption text={source.segment.text} language="en" />
            </div>
          )}
          {sourceInProgress && (
            <div className="strip-original-row strip-next-source">
              <span>当前原文</span>
              <TailCaption text={sourceInProgress.segment.text} language="en" />
            </div>
          )}
        </div>
      )}
    </div>
  );
}

function CaptionLane({
  lane,
  captions,
  onEdit,
}: {
  lane: CaptionSegment["lane"];
  captions: CaptionState;
  onEdit: (segment: CaptionSegment) => void;
}) {
  const [follow, setFollow] = useState(true);
  const listRef = useRef<HTMLDivElement>(null);
  const translation = lane === "translation";
  const confirmed = captions.confirmed.filter(
    (segment) => segment.lane === lane,
  );
  const drafts = captions.drafts.filter((segment) => segment.lane === lane);
  const segments = [...confirmed, ...drafts];
  const hasSpeaker = segments.some((segment) => segment.speaker);
  useLayoutEffect(() => {
    const list = listRef.current;
    if (list && follow) list.scrollTop = list.scrollHeight;
  }, [captions, follow]);

  return (
    <section
      className={`caption-lane ${lane}`}
      aria-label={translation ? "中文字幕" : "英文原文"}
    >
      <div className="lane-heading">
        <span className="lane-language">{translation ? "中" : "EN"}</span>
        {translation ? "中文翻译" : "英文原文"}
        {segments.length > 0 && !hasSpeaker && (
          <span className="lane-note">未分人</span>
        )}
        {drafts.length > 0 && (
          <span className="pending">{translation ? "修订中" : "生成中"}</span>
        )}
      </div>
      <div
        ref={listRef}
        className="caption-list"
        onScroll={(event) => {
          const list = event.currentTarget;
          setFollow(
            list.scrollHeight - list.scrollTop - list.clientHeight < 18,
          );
        }}
      >
        {segments.map((segment, index) => {
          const draft = index >= confirmed.length;
          const speakerChanged =
            index > 0 && segments[index - 1].speaker !== segment.speaker;
          const showSpeaker = hasSpeaker && (index === 0 || speakerChanged);
          return (
            <article
              key={segment.id}
              className={`caption ${draft ? "interim" : ""} ${speakerChanged ? "speaker-change" : ""}`}
            >
              {showSpeaker && segment.speaker && (
                <div className="speaker-line">
                  <span className="speaker">
                    <span />
                    {speakerLabel(captions, segment.speaker)}
                  </span>
                </div>
              )}
              <div className="caption-line">
                <p lang={translation ? "zh-Hans" : "en"}>
                  {segment.text.trim()}
                </p>
                {!draft && (
                  <IconButton
                    label={`编辑${translation ? "中文" : "英文"}字幕`}
                    onClick={() => onEdit(segment)}
                    className="edit-caption"
                  >
                    <Pencil size={13} />
                  </IconButton>
                )}
              </div>
            </article>
          );
        })}
        {segments.length === 0 && (
          <p className="lane-empty">等待{translation ? "翻译" : "原文"}…</p>
        )}
      </div>
      {!follow && (
        <button
          className="jump-button"
          type="button"
          onClick={() => setFollow(true)}
        >
          <ArrowDown size={14} /> 跳到最新
        </button>
      )}
    </section>
  );
}

function displayDate(value: string) {
  const date = new Date(
    value.includes("T") ? value : value.replace(" ", "T") + "Z",
  );
  return Number.isNaN(date.valueOf()) ? value : date.toLocaleString("zh-CN");
}

export default App;

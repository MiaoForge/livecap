import { useEffect, useRef, useState } from "react";
import type { ReactNode } from "react";
import { check } from "@tauri-apps/plugin-updater";
import type { Update } from "@tauri-apps/plugin-updater";
import { api, desktop } from "./clientApi";
import type { Settings, Provider } from "./clientApi";
export const languages = [
  ["en", "英语"],
  ["zh", "简体中文"],
  ["ja", "日语"],
  ["ko", "韩语"],
  ["fr", "法语"],
  ["de", "德语"],
  ["es", "西班牙语"],
  ["it", "意大利语"],
  ["pt", "葡萄牙语"],
  ["ru", "俄语"],
];
export const languageName = (code: string) =>
  languages.find((l) => l[0] === code)?.[1] || code;
export function Segments<T extends string>({
  value,
  options,
  onChange,
  disabled = false,
}: {
  value: T;
  options: [T, string][];
  onChange: (v: T) => void;
  disabled?: boolean;
}) {
  return (
    <div className="segmented">
      {options.map(([v, label]) => (
        <button
          type="button"
          key={v}
          className={value === v ? "active" : ""}
          aria-pressed={value === v}
          disabled={disabled}
          onClick={() => onChange(v)}
        >
          {label}
        </button>
      ))}
    </div>
  );
}
function Row({
  title,
  description,
  children,
}: {
  title: string;
  description?: string;
  children: ReactNode;
}) {
  return (
    <div className="product-setting-row">
      <div>
        <strong>{title}</strong>
        {description && <small>{description}</small>}
      </div>
      <div>{children}</div>
    </div>
  );
}
const displayVersion = (version: string) =>
  `v${version.replace(/-rc\.(\d+)$/, "-rc$1")}`;

function GeneralSettings({ active }: { active: boolean }) {
  const [info, setInfo] = useState<{
    version: string;
    configured: boolean;
  } | null>(null);
  const [message, setMessage] = useState("");
  const [busy, setBusy] = useState<"check" | "install" | null>(null);
  const [update, setUpdate] = useState<Update | null>(null);
  const installing = useRef(false);
  useEffect(() => {
    if (desktop)
      void api<{ version: string; configured: boolean }>("app_update_info")
        .then(setInfo)
        .catch(() => setMessage("无法读取版本信息。"));
  }, []);
  useEffect(
    () => () => {
      if (update && !installing.current) void update.close().catch(() => {});
    },
    [update],
  );

  async function checkForUpdates() {
    setBusy("check");
    setUpdate(null);
    setMessage("正在检查更新…");
    try {
      const next = await check({ timeout: 15000 });
      setUpdate(next);
      setMessage(
        next
          ? `发现新版本 ${displayVersion(next.version)}。安装时应用会退出。`
          : "当前已是最新版本。",
      );
    } catch {
      setMessage("检查更新失败，请稍后重试。");
    } finally {
      setBusy(null);
    }
  }

  async function installUpdate() {
    if (!update || active) return;
    installing.current = true;
    setBusy("install");
    setMessage("正在下载更新…");
    try {
      await update.downloadAndInstall((event) => {
        if (event.event === "Finished") setMessage("正在验证并安装更新…");
      });
    } catch {
      setMessage("更新安装失败，请重试。");
    } finally {
      installing.current = false;
      setBusy(null);
    }
  }

  const status =
    message ||
    (!desktop
      ? "请在桌面客户端检查更新。"
      : info && !info.configured
        ? "此版本尚未配置更新源。"
        : active
          ? "请在会话结束后安装更新。"
          : "");
  return (
    <>
      <h2>通用</h2>
      <Row title="当前版本">
        <span>
          {info ? displayVersion(info.version) : desktop ? "—" : "浏览器预览"}
        </span>
      </Row>
      <Row title="软件更新" description={status}>
        <button
          className="small"
          disabled={!info?.configured || busy !== null}
          onClick={() => void checkForUpdates()}
        >
          检查更新
        </button>
        <button
          className="primary small"
          disabled={!update || active || busy !== null}
          onClick={() => void installUpdate()}
        >
          下载安装
        </button>
      </Row>
    </>
  );
}
export function CaptionPreview({ settings: s }: { settings: Settings }) {
  const target =
    s.targetLanguage === "zh"
      ? "把注意力留给对话，让字幕跟上每一句。"
      : s.targetLanguage === "ja"
        ? "会話に集中して、一言ずつ字幕で理解しましょう。"
        : s.targetLanguage === "en"
          ? "Focus on the conversation. Follow every word."
          : languageName(s.targetLanguage) + " · 译文预览";
  const source = "Focus on the conversation. Follow every word.";
  return (
    <div
      className={"product-caption-sample " + s.captionTheme}
      style={{
        backgroundColor:
          s.captionTheme === "dark"
            ? `rgba(37,44,39,${s.captionOpacity})`
            : `rgba(255,255,255,${s.captionOpacity})`,
      }}
    >
      <div className="sample-speaker">
        ● 说话人 1 <span>字幕预览</span>
      </div>
      <div
        className={s.translationFirst ? "" : "source-first"}
        style={{ fontSize: s.fontSize }}
      >
        {s.captionMode !== "original" && (
          <p className="sample-target">{target}</p>
        )}
        {s.captionMode !== "translation" && (
          <p className="sample-source">{source}</p>
        )}
      </div>
    </div>
  );
}
export function ProductSettings({
  settings: s,
  setSettings,
  keys,
  onKeys,
  active,
  preview,
  fail,
  initialTab = "general",
  onSaved,
}: {
  settings: Settings;
  setSettings: (s: Settings) => void;
  keys: Partial<Record<Provider, boolean>>;
  onKeys: (keys: Partial<Record<Provider, boolean>>) => void;
  active: boolean;
  preview: () => void;
  fail: (e: unknown) => void;
  initialTab?: string;
  onSaved?: () => void;
}) {
  const [tab, setTab] = useState(initialTab),
    [key, setKey] = useState(""),
    [editingKey, setEditingKey] = useState(false),
    [working, setWorking] = useState(false),
    [saved, setSaved] = useState(false),
    [storage, setStorage] = useState<{
      dataDirectory: string;
      recordingCount: number;
      recordingBytes: number;
    } | null>(null);
  const latest = useRef(s),
    dirty = useRef(false),
    mounted = useRef(false);
  latest.current = s;
  useEffect(() => {
    if (tab === "storage" && desktop)
      void api<{
        dataDirectory: string;
        recordingCount: number;
        recordingBytes: number;
      }>("storage_info")
        .then(setStorage)
        .catch(fail);
  }, [tab]);
  useEffect(() => {
    if (!desktop) return;
    if (!mounted.current) {
      mounted.current = true;
      return;
    }
    dirty.current = true;
    setSaved(false);
    const timer = setTimeout(() => {
      void api("save_settings", { settings: s })
        .then(() => {
          if (latest.current === s) {
            dirty.current = false;
            setSaved(true);
            onSaved?.();
          }
        })
        .catch(fail);
    }, 350);
    return () => clearTimeout(timer);
  }, [s]);
  useEffect(
    () => () => {
      if (desktop && dirty.current)
        void api("save_settings", { settings: latest.current }).catch(fail);
    },
    [],
  );
  const change = (patch: Partial<Settings>) => setSettings({ ...s, ...patch });
  return (
    <div className="product-settings">
      <nav className="settings-subnav" aria-label="设置分类">
        {[
          ["general", "通用"],
          ["appearance", "字幕外观"],
          ["language", "语言与翻译"],
          ["storage", "知识与存储"],
          ["connection", "连接服务"],
        ].map(([id, label]) => (
          <button
            key={id}
            className={id === tab ? "active" : ""}
            aria-current={id === tab ? "page" : undefined}
            onClick={() => setTab(id)}
          >
            {label}
          </button>
        ))}
      </nav>
      <div className="settings-content">
        <section className="settings-surface">
          <div hidden={tab !== "general"}>
            <GeneralSettings active={active} />
            <Row
              title="会议麦克风"
              description="会议翻译默认同时采集系统声音和麦克风。关闭后仅采集系统声音。"
            >
              <button
                type="button"
                role="switch"
                aria-checked={s.meetingMicrophone}
                aria-label="会议麦克风"
                disabled={active}
                className={
                  "product-switch " + (s.meetingMicrophone ? "on" : "")
                }
                onClick={() =>
                  change({ meetingMicrophone: !s.meetingMicrophone })
                }
              >
                <i />
              </button>
            </Row>
          </div>
          {tab === "appearance" && (
            <>
              <h2>字幕外观</h2>
              <Row title="主题">
                <Segments
                  value={s.captionTheme}
                  options={[
                    ["dark", "深色"],
                    ["light", "浅色"],
                  ]}
                  onChange={(captionTheme) => change({ captionTheme })}
                />
              </Row>
              <Row title="字号">
                <div className="range-setting">
                  <input
                    aria-label="字幕字号"
                    type="range"
                    min={18}
                    max={36}
                    step={2}
                    value={s.fontSize}
                    onChange={(e) =>
                      change({ fontSize: Number(e.target.value) })
                    }
                  />
                  <output>{s.fontSize}</output>
                </div>
              </Row>
              <Row title="背景不透明度">
                <div className="range-setting">
                  <input
                    aria-label="背景不透明度"
                    type="range"
                    min={0.6}
                    max={1}
                    step={0.05}
                    value={s.captionOpacity}
                    onChange={(e) =>
                      change({ captionOpacity: Number(e.target.value) })
                    }
                  />
                  <output>{Math.round(s.captionOpacity * 100)}%</output>
                </div>
              </Row>
              <Row title="始终置顶" description="字幕窗显示在其他应用上方">
                <button
                  role="switch"
                  aria-checked={s.alwaysOnTop}
                  aria-label="字幕窗始终置顶"
                  className={"product-switch " + (s.alwaysOnTop ? "on" : "")}
                  onClick={() => change({ alwaysOnTop: !s.alwaysOnTop })}
                >
                  <i />
                </button>
              </Row>
            </>
          )}
          {tab === "language" && (
            <>
              <h2>语言与翻译</h2>
              <Row
                title="翻译为"
                description="自动识别原文语言，统一翻译为所选语言"
              >
                <select
                  aria-label="目标语言"
                  disabled={active}
                  value={s.targetLanguage}
                  onChange={(e) => change({ targetLanguage: e.target.value })}
                >
                  {languages.map(([value, label]) => (
                    <option value={value} key={value}>
                      {label}
                    </option>
                  ))}
                </select>
              </Row>
              <Row title="字幕内容">
                <Segments
                  value={s.captionMode}
                  options={[
                    ["both", "双语"],
                    ["translation", "仅译文"],
                    ["original", "仅原文"],
                  ]}
                  onChange={(captionMode) => change({ captionMode })}
                />
              </Row>
              <Row title="阅读顺序">
                <Segments
                  value={s.translationFirst ? "target" : "source"}
                  options={[
                    ["target", "译文在上"],
                    ["source", "原文在上"],
                  ]}
                  disabled={s.captionMode !== "both"}
                  onChange={(v) => change({ translationFirst: v === "target" })}
                />
              </Row>
            </>
          )}
          {tab === "connection" && (
            <>
              <h2>连接服务</h2>
              <div className="provider-options">
                {(["openai", "soniox"] as Provider[]).map((provider) => (
                  <button
                    key={provider}
                    disabled={active}
                    className={s.provider === provider ? "selected" : ""}
                    onClick={() => {
                      change({ provider });
                      setKey("");
                      setEditingKey(false);
                    }}
                  >
                    <strong>
                      {provider === "openai" ? "OpenAI" : "Soniox"}
                    </strong>
                    <small>
                      {provider === "openai"
                        ? "实时转录 + Luna 翻译"
                        : "实时转录、翻译与分人"}
                    </small>
                    <span>{keys[provider] ? "● 已配置" : "未配置"}</span>
                  </button>
                ))}
              </div>
              <Row title="API Key" description="保存在系统安全凭据存储">
                <span
                  className={
                    "connection-status " + (keys[s.provider] ? "connected" : "")
                  }
                >
                  {keys[s.provider] ? "● 已安全保存" : "○ 尚未配置"}
                </span>
                <button
                  className="small"
                  onClick={() => setEditingKey(!editingKey)}
                >
                  {keys[s.provider] ? "更换密钥" : "配置密钥"}
                </button>
              </Row>
              {(editingKey || !keys[s.provider]) && (
                <form
                  className="key-editor"
                  onSubmit={async (e) => {
                    e.preventDefault();
                    setWorking(true);
                    try {
                      await api("save_api_key", { provider: s.provider, key });
                      onKeys({ ...keys, [s.provider]: true });
                      setKey("");
                      setEditingKey(false);
                    } catch (err) {
                      fail(err);
                    } finally {
                      setWorking(false);
                    }
                  }}
                >
                  <input
                    aria-label="API Key"
                    autoComplete="off"
                    type="password"
                    placeholder="粘贴 API Key"
                    value={key}
                    onChange={(e) => setKey(e.target.value)}
                  />
                  <button
                    aria-describedby="key-save-help"
                    disabled={!desktop || !key.trim() || working}
                  >
                    保存密钥
                  </button>
                  <small id="key-save-help" role="status">
                    {!desktop
                      ? "请在桌面客户端中保存密钥。"
                      : working
                        ? "正在安全保存…"
                        : !key.trim()
                          ? "输入 API Key 后即可保存。"
                          : "密钥将保存到系统安全凭据存储。"}
                  </small>
                </form>
              )}
              <Row title="服务地址">
                <code>
                  {s.provider === "openai"
                    ? "api.openai.com"
                    : "api.soniox.com"}
                </code>
              </Row>
              <Row title="转录模型">
                <span>
                  {s.provider === "openai" ? "gpt-live-transcribe" : s.model}
                </span>
              </Row>
              {s.provider === "openai" && (
                <Row title="翻译与纪要模型">
                  <span>gpt-6-luna</span>
                </Row>
              )}
            </>
          )}
          {tab === "storage" && (
            <>
              <h2>知识与存储</h2>
              <Row title="字幕翻译" description="结束后清空音频与文字">
                <span className="quiet-badge">仅保留用量</span>
              </Row>
              <Row title="会议翻译" description="录音、文字与纪要保存在本机">
                <span className="quiet-badge">自动归档</span>
              </Row>
              <Row title="录音空间">
                <span>
                  {storage
                    ? `${storage.recordingCount} 个文件 · ${(storage.recordingBytes / 1024 / 1024).toFixed(1)} MB`
                    : "正在读取…"}
                </span>
              </Row>
              <Row title="数据位置">
                <button
                  className="small"
                  disabled={!desktop}
                  onClick={() => void api("open_storage_folder").catch(fail)}
                >
                  打开文件夹 ↗
                </button>
              </Row>
              {storage && (
                <p className="storage-path">{storage.dataDirectory}</p>
              )}
              <Row
                title="领域知识"
                description="两种模式共用本地领域资料与术语"
              >
                <span className="quiet-badge">本地缓存</span>
              </Row>
              <Row
                title="自动生成会议纪要"
                description="归档时用目标语言生成纪要。关闭后仍会自动生成标题和关键时间线。"
              >
                <button
                  type="button"
                  role="switch"
                  aria-checked={s.autoGenerateMeetingNotes}
                  aria-label="自动生成会议纪要"
                  disabled={active}
                  className={
                    "product-switch " + (s.autoGenerateMeetingNotes ? "on" : "")
                  }
                  onClick={() =>
                    change({
                      autoGenerateMeetingNotes: !s.autoGenerateMeetingNotes,
                    })
                  }
                >
                  <i />
                </button>
              </Row>
              <Row
                title="会后自动补充术语"
                description="OpenAI 会议在目标语言为中文时，结束后，将部分原文发给 Luna 识别候选。仅候选名称用于公开检索，查证后自动存入知识空间。会产生模型和搜索费用。"
              >
                <button
                  type="button"
                  role="switch"
                  aria-checked={s.autoLearnMeetingTerms}
                  aria-label="会后自动补充术语"
                  disabled={active}
                  className={
                    "product-switch " + (s.autoLearnMeetingTerms ? "on" : "")
                  }
                  onClick={() =>
                    change({ autoLearnMeetingTerms: !s.autoLearnMeetingTerms })
                  }
                >
                  <i />
                </button>
              </Row>
            </>
          )}
        </section>
        {["appearance", "language"].includes(tab) && (
          <section className="settings-preview">
            <div>
              <span>字幕预览</span>
              <button
                className="ghost small"
                disabled={!desktop}
                onClick={preview}
              >
                打开独立字幕窗 ↗
              </button>
            </div>
            <CaptionPreview settings={s} />
          </section>
        )}
        <div className="settings-autosave" role="status">
          {desktop && saved ? "✓ 已自动保存" : ""}
        </div>
      </div>
    </div>
  );
}

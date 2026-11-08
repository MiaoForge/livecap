import { useEffect, useMemo, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { ChevronRight, Sparkles } from "lucide-react";
import { api, desktop, date, money } from "./clientApi";
import type { Knowledge, KnowledgeProgress, Settings } from "./clientApi";
export type LearnedTerm = {
  term: string;
  occurrences: number;
  active: boolean;
};
const domains = [
  [
    "编程",
    "⌘",
    "代码、框架与工程实践",
    ["LLVM / Clang", "Web 开发", "人工智能"],
  ],
  ["设计", "◇", "产品、视觉与交互", ["产品设计", "设计系统", "用户研究"]],
  ["商业", "↗", "战略、市场与团队", ["产品策略", "市场营销", "管理"]],
  ["科研", "⌬", "论文、方法与学术交流", ["计算机科学", "生物医学", "材料科学"]],
  ["语言学习", "文", "学习、表达与文化", ["日常会话", "商务表达", "学术英语"]],
  ["自定义", "＋", "准备你关心的领域", []],
] as const;
const termOrder = new Intl.Collator("en", { sensitivity: "base" });
const termsPerPage = 40;
const initialProgress: KnowledgeProgress = {
  stage: "planning",
  area: null,
  completed: 0,
  total: 1,
};
export function ProductKnowledge({
  cache,
  onCache,
  hasKey,
  active,
  fail,
  onGenerating,
  openSettings,
  learned,
  settings,
  onSettings,
}: {
  cache: Knowledge | null;
  onCache: (k: Knowledge) => void;
  hasKey: boolean;
  active: boolean;
  fail: (e: unknown) => void;
  onGenerating: (b: boolean) => void;
  openSettings: () => void;
  learned: LearnedTerm[];
  settings: Settings;
  onSettings: (s: Settings) => void;
}) {
  const [tab, setTab] = useState("library"),
    [preparing, setPreparing] = useState(!cache),
    [domain, setDomain] = useState("编程"),
    [focus, setFocus] = useState(""),
    [query, setQuery] = useState(""),
    [selected, setSelected] = useState<string | null>(null),
    [generating, setGenerating] = useState(false),
    [working, setWorking] = useState(false);
  const [showTerms, setShowTerms] = useState(false),
    [termPage, setTermPage] = useState(0);
  const [progress, setProgress] = useState(initialProgress);
  const hasCache = Boolean(cache);
  useEffect(() => {
    if (hasCache) setPreparing(false);
  }, [hasCache]);
  const [result, setResult] = useState<{
    message: string;
    cancelled: boolean;
  } | null>(null);
  const broadLlvm =
    domain === "编程" && focus.trim().toLowerCase() === "llvm / clang";
  const activeStep =
    progress.stage === "planning"
      ? 0
      : ["researching", "supplementing", "reading-documents"].includes(
            progress.stage,
          )
        ? 1
        : 2;
  const progressTitle = (
    {
      planning: "正在规划覆盖范围",
      researching: "正在检索子主题",
      supplementing: "正在补充覆盖缺口",
      extracting: "正在整理词条与来源",
      saving: "正在写入本地缓存",
      "reading-documents": "正在读取官方文档",
      "extracting-documents": "正在逐批提取术语",
    } as const
  )[progress.stage];
  const progressDetail =
    progress.stage === "reading-documents"
      ? `${progress.completed} / ${progress.total} 份文档`
      : progress.stage === "extracting-documents"
        ? `${progress.completed} / ${progress.total} 批正文`
        : progress.area
          ? `${progress.completed} / ${progress.total} 个主题`
          : progress.stage === "planning"
            ? broadLlvm
              ? "12 个主题"
              : "4–6 个主题"
            : progress.stage === "saving"
              ? "本地写入"
              : "整理与校验";
  const ordered = useMemo(
    () =>
      showTerms
        ? [...(cache?.entries ?? [])].sort((a, b) =>
            termOrder.compare(a.term, b.term),
          )
        : [],
    [cache, showTerms],
  );
  const needle = query.trim().toLowerCase();
  const filtered = needle
    ? ordered.filter((entry) =>
        [
          entry.term,
          ...(entry.aliases ?? []),
          entry.translation,
          entry.meaning,
        ].some((value) => value.toLowerCase().includes(needle)),
      )
    : ordered;
  const pageCount = Math.max(1, Math.ceil(filtered.length / termsPerPage));
  const pageIndex = Math.min(termPage, pageCount - 1);
  const pageStart = pageIndex * termsPerPage;
  const visibleTerms = filtered.slice(pageStart, pageStart + termsPerPage);
  const entry = visibleTerms.find((e) => e.term === selected);
  function openTermList() {
    setQuery("");
    setSelected(null);
    setTermPage(0);
    setShowTerms(true);
  }
  async function generate() {
    if (generating || !desktop) return;
    let latestProgress = initialProgress;
    let unlisten: (() => void) | undefined;
    setResult(null);
    setProgress(initialProgress);
    setGenerating(true);
    onGenerating(true);
    try {
      unlisten = await listen<KnowledgeProgress>(
        "knowledge-progress",
        ({ payload }) => {
          latestProgress = payload;
          setProgress(payload);
        },
      );
      const next = await api<Knowledge>("generate_knowledge", {
        request: { domain, focus, sourceUrls: [] },
        consent: true,
      });
      onCache(next);
      setShowTerms(false);
      setTab("library");
      setPreparing(false);
    } catch (reason) {
      const message = reason instanceof Error ? reason.message : String(reason);
      const cancelled = message.startsWith("知识缓存生成已取消");
      const stage = (
        {
          planning: "规划范围",
          researching: "检索资料",
          supplementing: "补充检索",
          extracting: "整理词条",
          saving: "保存知识库",
          "reading-documents": "读取官方文档",
          "extracting-documents": "逐批提取术语",
        } as const
      )[latestProgress.stage];
      const area = latestProgress.area ? `（${latestProgress.area}）` : "";
      setResult({
        message:
          cancelled || !unlisten
            ? message
            : `最后进度：${stage}${area}。${message}`,
        cancelled,
      });
    } finally {
      unlisten?.();
      setGenerating(false);
      onGenerating(false);
    }
  }
  async function keep(term: string) {
    setWorking(true);
    try {
      const next = {
        ...settings,
        terms: Array.from(new Set([...settings.terms, term])),
      };
      await api("save_settings", { settings: next });
      onSettings(next);
    } catch (e) {
      fail(e);
    } finally {
      setWorking(false);
    }
  }
  return (
    <>
      <div className="heading-row">
        <h1>
          {!preparing && tab === "library" && showTerms ? "术语表" : "知识空间"}
        </h1>
        {!preparing && tab === "library" && showTerms && (
          <button className="ghost" onClick={() => setShowTerms(false)}>
            ← 返回领域资料
          </button>
        )}
        {cache && !preparing && !(tab === "library" && showTerms) && (
          <button
            onClick={() => {
              setResult(null);
              setPreparing(true);
            }}
          >
            ＋ 准备领域
          </button>
        )}
        {cache && preparing && !generating && (
          <button className="ghost" onClick={() => setPreparing(false)}>
            返回知识库
          </button>
        )}
      </div>
      {active && (
        <p className="knowledge-session-notice" role="status">
          会议进行中，也可以准备知识库。本场会话使用启动时的知识与热词，新库从下一场会话生效。
        </p>
      )}
      {preparing ? (
        <div className="knowledge-prepare">
          {result && (
            <div
              className={result.cancelled ? "subtitle" : "native-error"}
              role={result.cancelled ? "status" : "alert"}
            >
              {result.message}
            </div>
          )}
          {generating ? (
            <div className="knowledge-progress">
              <div className="preparation-header">
                <div className="preparation-orbit">
                  <Sparkles />
                </div>
                <div>
                  <span className="progress-kicker">领域知识准备</span>
                  <h2>正在准备{domain}知识</h2>
                  <p>
                    {broadLlvm
                      ? "按官方文档章节分批提取术语。"
                      : "逐主题检索并阅读资料，再整理词库。"}
                  </p>
                </div>
              </div>
              <div className="preparation-current" aria-live="polite">
                <div>
                  <span>当前阶段</span>
                  <strong>{progressTitle}</strong>
                  {progress.area && <p>{progress.area}</p>}
                </div>
                <b>{progressDetail}</b>
              </div>
              <progress
                aria-label="知识准备进度"
                max={Math.max(progress.total, 1)}
                value={progress.completed}
              />
              <div className="preparation-steps">
                {[
                  ["规划范围", "确定覆盖结构"],
                  ["逐主题检索", "搜索并阅读资料"],
                  ["整理与缓存", "去重、校验并保存"],
                ].map(([label, description], index) => (
                  <div
                    key={label}
                    className={
                      index < activeStep
                        ? "done"
                        : index === activeStep
                          ? "active"
                          : ""
                    }
                  >
                    <span>{index < activeStep ? "✓" : index + 1}</span>
                    <div>
                      <strong>{label}</strong>
                      <small>{description}</small>
                    </div>
                    <b>
                      {index < activeStep
                        ? "已完成"
                        : index === activeStep
                          ? "进行中"
                          : "等待"}
                    </b>
                  </div>
                ))}
              </div>
              <div className="preparation-footer">
                <span>可随时取消，已发出的请求仍可能计费。</span>
                <button
                  onClick={() => void api("cancel_knowledge").catch(fail)}
                >
                  取消生成
                </button>
              </div>
            </div>
          ) : (
            <>
              <div className="intro">
                <h2>准备你关心的领域</h2>
                <p className="subtitle">选择内容方向，让专业表达更准确。</p>
              </div>
              <div className="domains">
                {domains.map(([name, icon, description]) => (
                  <button
                    className={"domain " + (domain === name ? "selected" : "")}
                    key={name}
                    onClick={() => {
                      setDomain(name);
                      setFocus("");
                    }}
                  >
                    <span className="domain-glyph">{icon}</span>
                    <strong>{name}</strong>
                    <small>{description}</small>
                    <span className="check">✓</span>
                  </button>
                ))}
              </div>
              <div className="focus-area">
                <div className="label-line">关注方向</div>
                <div className="chips">
                  {domains
                    .find((d) => d[0] === domain)?.[3]
                    .map((f) => (
                      <button
                        className={"chip " + (focus === f ? "selected" : "")}
                        key={f}
                        onClick={() => setFocus(f)}
                      >
                        {f}
                      </button>
                    ))}
                </div>
                <input
                  className="knowledge-focus-input"
                  value={focus}
                  onChange={(e) => setFocus(e.target.value)}
                  placeholder="进一步描述关注方向，例如编译器前端与类型系统"
                />
              </div>
              <div className="setup-footer">
                <p className="hint">
                  {broadLlvm
                    ? "自动读取 LLVM / Clang 官方文档正文，再分批提取术语。正文会发给 OpenAI 整理，会产生模型费用，可能耗时数分钟。"
                    : "按 4–6 个子主题检索并阅读公开资料，必要时补搜一轮。会产生多轮模型与搜索费用，可能耗时数分钟。"}
                  结果仅保存到本机。
                </p>
                {hasKey ? (
                  <button
                    className="primary"
                    disabled={!desktop}
                    onClick={() => void generate()}
                  >
                    生成知识库 →
                  </button>
                ) : (
                  <button className="primary" onClick={openSettings}>
                    先连接 OpenAI →
                  </button>
                )}
              </div>
            </>
          )}
        </div>
      ) : (
        <>
          <div className="knowledge-tabs">
            <button
              className={tab === "library" ? "active" : ""}
              onClick={() => setTab("library")}
            >
              领域资料 <span>{cache?.entries.length || 0}</span>
            </button>
            <button
              className={tab === "learning" ? "active" : ""}
              onClick={() => setTab("learning")}
            >
              学习积累 <span>{settings.terms.length}</span>
            </button>
          </div>
          {tab === "library" ? (
            showTerms ? (
              <section aria-label="领域术语表">
                <div className="knowledge-toolbar">
                  <span role="status">
                    {cache?.focus || cache?.domain} · {filtered.length} 条术语
                  </span>
                  <input
                    type="search"
                    aria-label="搜索术语"
                    value={query}
                    onChange={(event) => {
                      setQuery(event.target.value);
                      setTermPage(0);
                      setSelected(null);
                    }}
                    placeholder="搜索术语、缩写或中文"
                  />
                </div>
                <div
                  className={"knowledge-browser " + (entry ? "has-detail" : "")}
                >
                  <div className="knowledge-term-table">
                    <table>
                      <thead>
                        <tr>
                          <th scope="col">术语（A–Z）</th>
                          <th scope="col">字幕译法</th>
                        </tr>
                      </thead>
                      <tbody>
                        {visibleTerms.map((term) => (
                          <tr
                            key={term.term}
                            className={selected === term.term ? "active" : ""}
                          >
                            <td>
                              <button
                                className="ghost"
                                type="button"
                                aria-expanded={selected === term.term}
                                onClick={() =>
                                  setSelected(
                                    selected === term.term ? null : term.term,
                                  )
                                }
                              >
                                {term.term}
                              </button>
                            </td>
                            <td>{term.translation}</td>
                          </tr>
                        ))}
                      </tbody>
                    </table>
                    {!visibleTerms.length && (
                      <div className="records-empty">没有匹配的术语</div>
                    )}
                    {pageCount > 1 && (
                      <nav
                        className="knowledge-pagination"
                        aria-label="术语分页"
                      >
                        <span>
                          第 {pageIndex + 1} / {pageCount} 页
                        </span>
                        <button
                          className="small"
                          disabled={pageIndex === 0}
                          onClick={() => {
                            setTermPage(pageIndex - 1);
                            setSelected(null);
                          }}
                        >
                          上一页
                        </button>
                        <button
                          className="small"
                          disabled={pageIndex === pageCount - 1}
                          onClick={() => {
                            setTermPage(pageIndex + 1);
                            setSelected(null);
                          }}
                        >
                          下一页
                        </button>
                      </nav>
                    )}
                  </div>
                  {entry && (
                    <aside className="knowledge-detail">
                      <button
                        className="ghost close"
                        aria-label="关闭术语详情"
                        onClick={() => setSelected(null)}
                      >
                        ×
                      </button>
                      <span className="eyebrow">领域词汇</span>
                      <h2>{entry.term}</h2>
                      <p>{entry.meaning}</p>
                      {!!entry.aliases?.length && (
                        <>
                          <h3>缩写与别名</h3>
                          <p>{entry.aliases.join(" · ")}</p>
                        </>
                      )}
                      <h3>字幕表达</h3>
                      <p>{entry.translation}</p>
                      <h3>资料来源</h3>
                      {entry.sourceUrls.map((url) => (
                        <button
                          key={url}
                          onClick={() =>
                            void api("open_knowledge_source", { url }).catch(
                              fail,
                            )
                          }
                        >
                          {cache?.sources.find((s) => s.url === url)?.title ||
                            new URL(url).hostname}{" "}
                          ↗
                        </button>
                      ))}
                    </aside>
                  )}
                </div>
                <div className="knowledge-footer">
                  <span>{cache?.sources.length || 0} 个资料来源</span>
                  <span>本次准备费用 {money(cache?.usage.estimatedUsd)}</span>
                </div>
              </section>
            ) : (
              <div className="knowledge-overview knowledge-overview-link">
                <button
                  type="button"
                  className="knowledge-open-terms"
                  onClick={openTermList}
                >
                  <span className="knowledge-emblem" aria-hidden="true">
                    ▦
                  </span>
                  <span className="knowledge-overview-name">
                    <strong>{cache?.domain}</strong>
                    <small>
                      {cache?.focus || "领域资料与专业表达"} · 更新于{" "}
                      {cache ? date(cache.updatedAt) : ""}
                    </small>
                  </span>
                  <span className="knowledge-overview-stats">
                    <b>{cache?.entries.length}</b>
                    <small>专业词汇</small>
                  </span>
                  <ChevronRight size={16} aria-hidden="true" />
                </button>
                <button
                  className={cache?.enabled ? "active small" : "small"}
                  aria-pressed={cache?.enabled ?? false}
                  onClick={() =>
                    void api<Knowledge>("set_knowledge_enabled", {
                      enabled: !cache?.enabled,
                    })
                      .then(onCache)
                      .catch(fail)
                  }
                >
                  {cache?.enabled ? "● 已启用" : "启用"}
                </button>
              </div>
            )
          ) : (
            <>
              <section className="learning-intro">
                <h2>从对话中积累专业表达</h2>
                <p>
                  会话中重复出现的技术词会成为候选；保留后，后续会话会继续使用。
                </p>
              </section>
              <h3 className="learning-heading">
                已保留 · {settings.terms.length}
              </h3>
              <div className="learned-grid">
                {settings.terms.map((t) => (
                  <div key={t}>
                    <b>{t}</b>
                    <small>个人词汇</small>
                  </div>
                ))}
              </div>
              {!settings.terms.length && (
                <div className="records-empty">还没有保留的词汇</div>
              )}
              <h3 className="learning-heading">
                本次会话候选 · {learned.length}
              </h3>
              <div className="learning-list">
                {learned.map((t) => (
                  <div key={t.term}>
                    <div>
                      <strong>{t.term}</strong>
                      <small>出现 {t.occurrences} 次</small>
                    </div>
                    <button
                      disabled={
                        working || settings.terms.includes(t.term) || !t.active
                      }
                      onClick={() => void keep(t.term)}
                    >
                      {settings.terms.includes(t.term) ? "已保留" : "保留词汇"}
                    </button>
                  </div>
                ))}
              </div>
              {!learned.length && (
                <p className="subtitle">开始翻译后，候选词汇会显示在这里。</p>
              )}
            </>
          )}
        </>
      )}
    </>
  );
}

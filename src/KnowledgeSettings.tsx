import { useEffect, useRef, useState } from "react";
import type { FormEvent, ReactNode } from "react";
import { invoke } from "@tauri-apps/api/core";
import { ExternalLink } from "lucide-react";

type KnowledgeRequest = { domain: string; focus: string; sourceUrls: string[] };
type KnowledgeSource = { url: string; title: string };
type KnowledgeEntry = {
  term: string;
  aliases: string[];
  meaning: string;
  translation: string;
  example: string;
  sourceUrls: string[];
};

type KnowledgeCache = {
  domain: string;
  focus: string;
  updatedAt: string;
  enabled: boolean;
  sources: KnowledgeSource[];
  entries: KnowledgeEntry[];
  usage: {
    inputTokens: number;
    cachedInputTokens: number;
    outputTokens: number;
    webSearchCalls: number;
    estimatedUsd: number | null;
    incomplete: boolean;
  };
};

type Props = {
  active: boolean;
  desktop: boolean;
  canGenerate: boolean;
  hasKey: boolean;
  onGenerationChange: (running: boolean) => void;
  Modal: (props: {
    title: string;
    children: ReactNode;
    onClose: () => void;
  }) => ReactNode;
  formatUsd: (value: number | null) => string;
  displayDate: (value: string) => string;
};

function isHttps(value: string) {
  try {
    const url = new URL(value);
    return (
      url.protocol === "https:" &&
      Boolean(url.hostname) &&
      !url.username &&
      !url.password
    );
  } catch {
    return false;
  }
}

function errorText(reason: unknown) {
  return reason instanceof Error ? reason.message : String(reason);
}

export function KnowledgeSettings({
  active,
  desktop,
  canGenerate,
  hasKey,
  onGenerationChange,
  Modal,
  formatUsd,
  displayDate,
}: Props) {
  const [cache, setCache] = useState<KnowledgeCache | null>(null);
  const [domain, setDomain] = useState("");
  const [focus, setFocus] = useState("");
  const [urls, setUrls] = useState("");
  const [query, setQuery] = useState("");
  const [loading, setLoading] = useState(desktop);
  const [working, setWorking] = useState(false);
  const [generating, setGenerating] = useState(false);
  const [canceling, setCanceling] = useState(false);
  const [request, setRequest] = useState<KnowledgeRequest | null>(null);
  const [confirmClear, setConfirmClear] = useState(false);
  const [error, setError] = useState("");
  const [notice, setNotice] = useState("");
  const loaded = useRef(false);

  useEffect(() => {
    if (!active || !desktop || loaded.current) return;
    let disposed = false;
    void invoke<KnowledgeCache | null>("load_knowledge")
      .then((saved) => {
        if (disposed) return;
        setCache(saved);
        if (saved) {
          setDomain(saved.domain);
          setFocus(saved.focus);
        }
      })
      .catch((reason) => {
        if (!disposed) setError(errorText(reason));
      })
      .finally(() => {
        if (!disposed) {
          loaded.current = true;
          setLoading(false);
        }
      });
    return () => {
      disposed = true;
    };
  }, [active, desktop]);

  function prepareGeneration(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    if (
      !desktop ||
      !canGenerate ||
      !hasKey ||
      generating ||
      working ||
      loading ||
      !domain.trim()
    )
      return;
    const sourceUrls = [
      ...new Set(
        urls
          .split(/\r?\n/)
          .map((url) => url.trim())
          .filter(Boolean),
      ),
    ];
    setError("");
    setNotice("");
    if (sourceUrls.length > 5 || sourceUrls.some((url) => !isHttps(url))) {
      setError("最多添加 5 个公开 HTTPS 参考链接，每行一个。");
      return;
    }
    setRequest({ domain: domain.trim(), focus: focus.trim(), sourceUrls });
  }

  async function generate() {
    if (!request || !canGenerate || generating) return;
    const currentRequest = request;
    setRequest(null);
    setGenerating(true);
    onGenerationChange(true);
    try {
      const result = await invoke<KnowledgeCache>("generate_knowledge", {
        request: currentRequest,
        consent: true,
      });
      setCache(result);
      setQuery("");
      setNotice(`知识缓存已更新，共 ${result.entries.length} 条。`);
    } catch (reason) {
      const message = errorText(reason);
      if (message.startsWith("知识缓存生成已取消")) setNotice(message);
      else
        setError(
          message.includes("计费")
            ? message
            : `${message} 已发出的请求仍可能计费。`,
        );
    } finally {
      setGenerating(false);
      setCanceling(false);
      onGenerationChange(false);
    }
  }

  async function cancel() {
    setCanceling(true);
    try {
      await invoke("cancel_knowledge");
    } catch (reason) {
      setCanceling(false);
      setError(errorText(reason));
    }
  }

  async function changeCache(task: () => Promise<KnowledgeCache | null>) {
    setWorking(true);
    setError("");
    setNotice("");
    try {
      setCache(await task());
    } catch (reason) {
      setError(errorText(reason));
    } finally {
      setWorking(false);
    }
  }

  async function openSource(url: string) {
    if (!desktop || !isHttps(url)) return;
    try {
      await invoke("open_knowledge_source", { url });
    } catch (reason) {
      try {
        await navigator.clipboard.writeText(url);
        setNotice("来源链接已复制。");
      } catch {
        setError(errorText(reason));
      }
    }
  }

  const disabled = loading || working || generating;
  const search = query.trim().toLowerCase();
  const entries =
    cache?.entries.filter(({ term, aliases, meaning, translation, example }) =>
      [term, ...aliases, meaning, translation, example].some((value) =>
        value.toLowerCase().includes(search),
      ),
    ) ?? [];

  function sourceButton(url: string) {
    if (!isHttps(url)) return null;
    const title = cache?.sources.find((source) => source.url === url)?.title;
    return (
      <li key={url}>
        <button
          className="knowledge-link"
          type="button"
          onClick={() => void openSource(url)}
          title={url}
          disabled={!desktop}
        >
          <ExternalLink size={12} />
          {title || url}
        </button>
      </li>
    );
  }

  return (
    <section className="knowledge-settings" aria-labelledby="knowledge-heading">
      <h2 id="knowledge-heading">使用领域与知识缓存</h2>
      <p className="knowledge-help">
        告诉 Livecap 你通常在听什么，Luna
        会查找公开资料并整理术语和背景。留空也可以使用字幕。
      </p>
      {error && (
        <p className="knowledge-error" role="alert">
          {error}
        </p>
      )}
      {notice && (
        <p className="knowledge-notice" role="status">
          {notice}
        </p>
      )}
      <form className="knowledge-form" onSubmit={prepareGeneration}>
        <div className="field">
          <label htmlFor="knowledge-domain">使用领域</label>
          <input
            id="knowledge-domain"
            value={domain}
            maxLength={120}
            required
            disabled={disabled}
            onChange={(event) => setDomain(event.target.value)}
            placeholder="例如 LLVM / Clang 编译器开发"
          />
        </div>
        <div className="field">
          <label htmlFor="knowledge-focus">
            关注方向 <span>可选</span>
          </label>
          <input
            id="knowledge-focus"
            value={focus}
            maxLength={600}
            disabled={disabled}
            onChange={(event) => setFocus(event.target.value)}
            placeholder="例如优化 pass、诊断与类型系统"
          />
        </div>
        <div className="field wide">
          <label htmlFor="knowledge-urls">
            参考链接 <span>可选，每行一个，最多 5 个公开 HTTPS 链接</span>
          </label>
          <textarea
            id="knowledge-urls"
            rows={2}
            value={urls}
            disabled={disabled}
            onChange={(event) => setUrls(event.target.value)}
            placeholder="例如项目文档或术语表"
          />
        </div>
        <div className="wide knowledge-actions">
          <span className="knowledge-help" role="status">
            {loading
              ? "正在读取本机缓存…"
              : generating
                ? "正在查找资料并整理知识缓存…"
                : !desktop
                  ? "在桌面端可生成缓存。"
                  : !hasKey
                    ? "请在下方保存 OpenAI 密钥后生成。"
                    : !canGenerate
                      ? "请等待当前操作结束。"
                      : !cache
                        ? "尚未生成知识缓存。"
                        : ""}
          </span>
          {generating ? (
            <button
              className="button subtle"
              type="button"
              disabled={canceling}
              onClick={() => void cancel()}
            >
              {canceling ? "正在取消…" : "取消生成"}
            </button>
          ) : (
            <button
              className="button primary"
              type="submit"
              disabled={
                !desktop ||
                !hasKey ||
                !canGenerate ||
                disabled ||
                !domain.trim()
              }
            >
              {cache ? "刷新知识缓存" : "生成知识缓存"}
            </button>
          )}
        </div>
      </form>

      {cache && (
        <div className="knowledge-cache">
          <div className="knowledge-cache-summary">
            <div>
              <strong>{cache.domain}</strong>
              <p>
                {cache.entries.length} 条知识 · {displayDate(cache.updatedAt)}{" "}
                更新
              </p>
            </div>
            <label className="check-label">
              <input
                type="checkbox"
                checked={cache.enabled}
                disabled={disabled}
                onChange={(event) => {
                  const enabled = event.target.checked;
                  void changeCache(() =>
                    invoke<KnowledgeCache>("set_knowledge_enabled", {
                      enabled,
                    }),
                  );
                }}
              />
              下次会话使用
            </label>
            <button
              className="button subtle"
              type="button"
              disabled={disabled}
              onClick={() => setConfirmClear(true)}
            >
              清除缓存
            </button>
          </div>
          <p className="knowledge-help">
            OpenAI 按字幕检索相关背景，Soniox 使用核心术语。
          </p>
          <details className="knowledge-inspector">
            <summary>查看知识条目和来源</summary>
            <p className="knowledge-help">
              释义、译法和例句由模型根据来源整理，可对照原文核实。
            </p>
            <div className="field">
              <label htmlFor="knowledge-search">搜索知识条目</label>
              <input
                id="knowledge-search"
                type="search"
                value={query}
                onChange={(event) => setQuery(event.target.value)}
              />
            </div>
            {entries.length ? (
              <div className="knowledge-results">
                {entries.map((entry) => (
                  <details key={entry.term} className="knowledge-entry">
                    <summary>
                      <strong>{entry.term}</strong>{" "}
                      {entry.translation && <span>{entry.translation}</span>}
                    </summary>
                    {entry.aliases.length > 0 && (
                      <p>别名 · {entry.aliases.join("、")}</p>
                    )}
                    {entry.meaning && <p>{entry.meaning}</p>}
                    {entry.example && <p>例句 · {entry.example}</p>}
                    {entry.sourceUrls.length > 0 && (
                      <ul className="knowledge-sources">
                        {entry.sourceUrls.map(sourceButton)}
                      </ul>
                    )}
                    <button
                      className="button subtle"
                      type="button"
                      disabled={disabled}
                      onClick={() =>
                        void changeCache(() =>
                          invoke<KnowledgeCache>("remove_knowledge_entry", {
                            term: entry.term,
                          }),
                        )
                      }
                    >
                      移除此条
                    </button>
                  </details>
                ))}
              </div>
            ) : (
              <p className="knowledge-help">没有符合条件的条目。</p>
            )}
            {cache.sources.length > 0 && (
              <details className="knowledge-all-sources">
                <summary>全部来源（{cache.sources.length}）</summary>
                <ul className="knowledge-sources">
                  {cache.sources.map(({ url }) => sourceButton(url))}
                </ul>
              </details>
            )}
          </details>
          <details className="knowledge-inspector">
            <summary>
              这份缓存的生成费用 ·{" "}
              {cache.usage.estimatedUsd === null
                ? "暂不可估"
                : formatUsd(cache.usage.estimatedUsd)}
            </summary>
            <p className="knowledge-help">
              搜索 {cache.usage.webSearchCalls} 次，输入{" "}
              {cache.usage.inputTokens.toLocaleString("zh-CN")} token （其中缓存{" "}
              {cache.usage.cachedInputTokens.toLocaleString("zh-CN")}），输出{" "}
              {cache.usage.outputTokens.toLocaleString("zh-CN")} token。 金额按
              API 价格估算，实际扣费以账单为准。
            </p>
            {cache.usage.incomplete && (
              <p className="knowledge-help">部分用量未取得，费用估算不完整。</p>
            )}
          </details>
        </div>
      )}

      {request && (
        <Modal
          title={cache ? "刷新知识缓存？" : "生成知识缓存？"}
          onClose={() => setRequest(null)}
        >
          <p>
            Livecap 会将领域、关注方向和参考链接发送给
            OpenAI，查找公开资料。最多 4 次搜索、2 次 Luna
            请求，按实际用量计费。结果保存到本机，可在后续会话复用。
          </p>
          <dl className="knowledge-review">
            <dt>领域</dt>
            <dd>{request.domain}</dd>
            {request.focus && (
              <>
                <dt>关注方向</dt>
                <dd>{request.focus}</dd>
              </>
            )}
            {request.sourceUrls.length > 0 && (
              <>
                <dt>参考链接</dt>
                <dd>{request.sourceUrls.join("\n")}</dd>
              </>
            )}
          </dl>
          <div className="modal-actions">
            <button
              className="button subtle"
              type="button"
              autoFocus
              onClick={() => setRequest(null)}
            >
              取消
            </button>
            <button
              className="button primary"
              type="button"
              disabled={!canGenerate}
              onClick={() => void generate()}
            >
              同意并生成
            </button>
          </div>
        </Modal>
      )}

      {confirmClear && (
        <Modal title="清除知识缓存？" onClose={() => setConfirmClear(false)}>
          <p>将删除本机保存的领域知识、来源和生成用量记录。</p>
          <div className="modal-actions">
            <button
              className="button subtle"
              type="button"
              autoFocus
              onClick={() => setConfirmClear(false)}
            >
              取消
            </button>
            <button
              className="button danger"
              type="button"
              onClick={() => {
                setConfirmClear(false);
                void changeCache(async () => {
                  await invoke("clear_knowledge");
                  return null;
                });
              }}
            >
              清除缓存
            </button>
          </div>
        </Modal>
      )}
    </section>
  );
}

import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useLayoutEffect,
  useState,
} from "react";
import type { ReactNode } from "react";
import { createPortal } from "react-dom";
import { X, AlertCircle, Info, TriangleAlert } from "lucide-react";

export type NoticeKind = "error" | "warning" | "info";
export type Notice = { id: number; message: string; kind: NoticeKind };
export function appendNotice(current: Notice[], next: Notice): Notice[] {
  return [
    ...current.filter(
      (item) => item.message !== next.message || item.kind !== next.kind,
    ),
    next,
  ].slice(-4);
}
const NotificationContext = createContext<
  (message: string, kind?: NoticeKind) => void
>(() => {});
export const useNotify = () => useContext(NotificationContext);

export function NotificationProvider({ children }: { children: ReactNode }) {
  const [notices, setNotices] = useState<Notice[]>([]);
  const notify = useCallback((message: string, kind: NoticeKind = "error") => {
    if (!message.trim()) return;
    setNotices((current) =>
      appendNotice(current, {
        id: (current[current.length - 1]?.id ?? 0) + 1,
        message,
        kind,
      }),
    );
  }, []);
  const dismiss = useCallback(
    (id: number) =>
      setNotices((current) => current.filter((item) => item.id !== id)),
    [],
  );
  return (
    <NotificationContext.Provider value={notify}>
      {children}
      <NotificationCenter notices={notices} dismiss={dismiss} />
    </NotificationContext.Provider>
  );
}
export function NoticeCard({
  notice,
  dismiss,
}: {
  notice: Notice;
  dismiss: (id: number) => void;
}) {
  const [paused, setPaused] = useState(false);
  useEffect(() => {
    if (paused || notice.kind === "error") return;
    const timer = window.setTimeout(
      () => dismiss(notice.id),
      notice.kind === "warning" ? 12000 : 8000,
    );
    return () => window.clearTimeout(timer);
  }, [notice, dismiss, paused]);
  const Icon =
    notice.kind === "error"
      ? AlertCircle
      : notice.kind === "warning"
        ? TriangleAlert
        : Info;
  return (
    <div
      className={`notice notice-${notice.kind}`}
      role={notice.kind === "error" ? "alert" : "status"}
      aria-atomic="true"
      onMouseEnter={() => setPaused(true)}
      onMouseLeave={() => setPaused(false)}
      onFocus={() => setPaused(true)}
      onBlur={(event) => {
        if (!event.currentTarget.contains(event.relatedTarget))
          setPaused(false);
      }}
    >
      <Icon aria-hidden="true" />
      <div>
        <strong>
          {
            { error: "操作未完成", warning: "请留意", info: "提示" }[
              notice.kind
            ]
          }
        </strong>
        <p>{notice.message}</p>
      </div>
      <button aria-label="关闭通知" onClick={() => dismiss(notice.id)}>
        <X />
      </button>
    </div>
  );
}
function NotificationCenter({
  notices,
  dismiss,
}: {
  notices: Notice[];
  dismiss: (id: number) => void;
}) {
  const [host, setHost] = useState<Element | null>(null);
  // A modal dialog is in the top layer; keep its notifications reachable there.
  useLayoutEffect(() => {
    const sync = () =>
      setHost(document.querySelector("dialog[open]") ?? document.body);
    sync();
    const observer = new MutationObserver(sync);
    observer.observe(document.body, {
      childList: true,
      subtree: true,
      attributes: true,
      attributeFilter: ["open"],
    });
    return () => observer.disconnect();
  }, []);
  return host
    ? createPortal(
        <section className="notification-center" aria-label="通知">
          {notices.map((notice) => (
            <NoticeCard key={notice.id} notice={notice} dismiss={dismiss} />
          ))}
        </section>,
        host,
      )
    : null;
}
export function ErrorNotice({
  message,
  kind = "error",
}: {
  message?: string | null;
  kind?: NoticeKind;
}) {
  const notify = useNotify();
  useEffect(() => {
    if (message) notify(message, kind);
  }, [message, kind, notify]);
  return null;
}

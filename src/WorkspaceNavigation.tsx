import type { ReactNode } from "react";
import {
  Home,
  FileText,
  BookOpen,
  Radio,
  ChartPie,
  Settings2,
} from "lucide-react";
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
export function WorkspaceNavigation({
  page,
  active,
  generating,
  recordCount,
  onNavigate,
}: {
  page: string;
  active: boolean;
  generating: boolean;
  recordCount: number;
  onNavigate: (page: string) => void;
}) {
  return (
    <>
      <nav aria-label="工作空间导航">
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
            aria-current={page === p ? "page" : undefined}
            onClick={(event) => {
              event.preventDefault();
              onNavigate(p);
            }}
          >
            <span className="nav-icon">{navIcons[p]}</span>
            {names[p]}
            {p === "records" && (
              <small className="nav-count">{recordCount}</small>
            )}
            {p === "knowledge" && generating && (
              <small className="nav-count">整理中</small>
            )}
          </a>
        ))}
        <a
          href="#settings"
          className={"mobile-settings " + (page === "settings" ? "active" : "")}
          aria-current={page === "settings" ? "page" : undefined}
          onClick={(event) => {
            event.preventDefault();
            onNavigate("settings");
          }}
        >
          设置
        </a>
      </nav>
      <div className="sidebar-bottom">
        <nav aria-label="设置导航">
          <a
            href="#settings"
            className={page === "settings" ? "active" : ""}
            aria-current={page === "settings" ? "page" : undefined}
            onClick={(event) => {
              event.preventDefault();
              onNavigate("settings");
            }}
          >
            <span className="nav-icon">
              <Settings2 />
            </span>
            设置
          </a>
        </nav>
      </div>
    </>
  );
}

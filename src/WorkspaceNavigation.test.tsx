import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import { WorkspaceNavigation } from "./WorkspaceNavigation";

function render(page = "home", active = false) {
  return renderToStaticMarkup(
    <WorkspaceNavigation
      page={page}
      active={active}
      generating={false}
      recordCount={3}
      onNavigate={() => {}}
    />,
  );
}

describe("workspace navigation accessibility", () => {
  it("distinguishes the workspace and settings navigation landmarks", () => {
    const labels = [...render().matchAll(/<nav aria-label="([^"]+)"/g)].map(
      (match) => match[1],
    );
    expect(labels).toEqual(["工作空间导航", "设置导航"]);
    expect(new Set(labels).size).toBe(labels.length);
  });

  it("keeps a settings entry in the main navigation for narrow screens", () => {
    const mainNavigation = render("settings").split("</nav>")[0];
    expect(mainNavigation).toMatch(
      /href="#settings" class="mobile-settings active" aria-current="page"/,
    );
  });

  it("exposes the current page and only offers the session while active", () => {
    expect(render("records")).toMatch(
      /href="#records" class="active" aria-current="page"/,
    );
    expect(render()).not.toContain('href="#session"');
    expect(render("session", true)).toMatch(
      /href="#session" class="active" aria-current="page"/,
    );
  });
});

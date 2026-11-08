import { renderToStaticMarkup } from "react-dom/server";
import { expect, it } from "vitest";
import { appendNotice, NoticeCard } from "./Notifications";
import type { Notice } from "./Notifications";

it("deduplicates repeated notices, retains the latest reason, and bounds the stack", () => {
  const initial: Notice[] = [1, 2, 3, 4].map((id) => ({
    id,
    kind: "error",
    message: `Error ${id}`,
  }));
  const result = appendNotice(initial, {
    id: 5,
    kind: "error",
    message: "Error 2",
  });
  expect(result.map((notice) => notice.id)).toEqual([1, 3, 4, 5]);
  expect(
    appendNotice(result, { id: 6, kind: "warning", message: "Try later" }).map(
      (notice) => notice.id,
    ),
  ).toEqual([3, 4, 5, 6]);
});
it.each(["error", "warning", "info"] as const)(
  "announces %s notifications and provides a labelled close action",
  (kind) => {
    const html = renderToStaticMarkup(
      <NoticeCard
        notice={{ id: 1, kind, message: "Service unavailable" }}
        dismiss={() => {}}
      />,
    );
    expect(html).toContain(`role="${kind === "error" ? "alert" : "status"}"`);
    expect(html).toContain('aria-label="关闭通知"');
    expect(html).toContain("Service unavailable");
  },
);

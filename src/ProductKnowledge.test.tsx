import { renderToStaticMarkup } from "react-dom/server";
import { expect, it, vi } from "vitest";
import { ProductKnowledge } from "./ProductKnowledge";
import { defaults } from "./clientApi";
import type { Knowledge } from "./clientApi";

vi.mock("./clientApi", async (importOriginal) => ({
  ...(await importOriginal<typeof import("./clientApi")>()),
  desktop: true,
}));

const noop = () => {};
function render(cache: Knowledge | null) {
  return renderToStaticMarkup(
    <ProductKnowledge
      cache={cache}
      onCache={noop}
      hasKey
      active
      fail={noop}
      onGenerating={noop}
      openSettings={noop}
      learned={[]}
      settings={defaults}
      onSettings={noop}
    />,
  );
}
function button(html: string, label: string) {
  return [...html.matchAll(/<button\b[^>]*>[\s\S]*?<\/button>/g)]
    .map((match) => match[0])
    .find((markup) => markup.includes(label));
}

it("allows starting knowledge preparation during an active meeting", () => {
  const html = render(null);
  const generate = button(html, "生成知识库");
  expect(generate).toBeDefined();
  expect(generate).not.toContain("disabled");
  expect(html).toContain("本场会话使用启动时的知识与热词");
  expect(html).toContain("下一场会话生效");
});

it("allows preparing another domain and changing future knowledge use during a meeting", () => {
  const cache: Knowledge = {
    domain: "编程",
    focus: "LLVM",
    enabled: true,
    updatedAt: "2026-11-08T00:00:00Z",
    entries: [],
    sources: [],
    usage: {
      inputTokens: 0,
      cachedInputTokens: 0,
      outputTokens: 0,
      webSearchCalls: 0,
      estimatedUsd: 0,
      incomplete: false,
    },
  };
  const html = render(cache);
  expect(button(html, "准备领域")).toBeDefined();
  expect(button(html, "准备领域")).not.toContain("disabled");
  expect(button(html, "已启用")).not.toContain("disabled");
});

it("shows independent saved domains and simultaneous enabled state", () => {
  const base: Knowledge = {
    domain: "编程",
    focus: "LLVM",
    enabled: true,
    updatedAt: "2026-11-08",
    entries: [],
    sources: [],
    usage: {
      inputTokens: 0,
      cachedInputTokens: 0,
      outputTokens: 0,
      webSearchCalls: 0,
      estimatedUsd: 0,
      incomplete: false,
    },
  };
  const html = renderToStaticMarkup(
    <ProductKnowledge
      cache={base}
      libraries={[
        { id: "compiler", cache: base },
        {
          id: "design",
          cache: { ...base, domain: "设计", focus: "界面", enabled: false },
        },
      ]}
      libraryId="compiler"
      onSelectLibrary={noop}
      onToggleLibrary={noop}
      onCache={noop}
      hasKey
      active
      fail={noop}
      onGenerating={noop}
      openSettings={noop}
      learned={[]}
      settings={defaults}
      onSettings={noop}
    />,
  );
  expect(html).toContain("个已启用 · 可同时使用");
  expect(html).toMatch(/aria-label="启用领域：编程 LLVM" aria-pressed="true"/);
  expect(html).toMatch(/aria-label="启用领域：设计 界面" aria-pressed="false"/);
  expect(html).toContain('aria-label="已保存的知识领域"');
});
